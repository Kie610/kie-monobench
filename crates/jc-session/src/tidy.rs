//! モデル管理の整理(ホームの左の「整理」): 同じ・似ているデザインの組、分類のないデザイン、
//! 見つからないファイル。画面・CLI・MCP が同じ関数を使う。読むだけで、ファイルを変えない。
//! どれも候補を並べるだけで、移す・消すのは利用者が選んだ時に [`crate::organize`] で行う。
//!
//! 似ている物は、中身の指紋の一致・名前の正規化・形の数(体積・外形の箱・面の数。版の履歴に
//! 保存の時に残した物)で比べる。縮小図の見た目では比べない(絵を部分に分けた
//! 知覚の指紋で塊にする形を避けるため、絵は使わない。docs/避ける形.md)。

use crate::history::{self, Shape};
use crate::projects::{ModelInfo, fold, info_of_json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 形が近いとみなす差(割合)。
pub const SHAPE_TOL: f64 = 0.02;

/// 整理に使うデザイン 1 つの値(ファイルを 1 回読んで作る)。
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    /// 拡張子を除いたファイル名
    pub name: String,
    /// 中身の指紋(JC が保存した物は最新の版の指紋と同じ)
    pub fingerprint: String,
    /// 版の数
    pub versions: usize,
    /// 形の数(いちばん新しい物。形の無い・古い版だけの物は無し)
    pub shape: Option<Shape>,
    pub info: ModelInfo,
    pub modified: Option<SystemTime>,
    pub bytes: u64,
}

/// `path` の値を読む(読めなければ `None`)。
pub fn entry(path: &Path) -> Option<Entry> {
    let json = std::fs::read_to_string(path).ok()?;
    let meta = std::fs::metadata(path).ok();
    let versions = history::list(path);
    Some(Entry {
        path: path.to_path_buf(),
        name: path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().to_string()),
        // 保存は JSON の後に改行を 1 つ足すので、それを除いて版と同じ指紋にする
        fingerprint: history::fingerprint(json.trim_end()),
        shape: versions.iter().find_map(|v| v.shape),
        versions: versions.len(),
        info: info_of_json(&json),
        modified: meta.as_ref().and_then(|m| m.modified().ok()),
        bytes: meta.map_or(0, |m| m.len()),
    })
}

/// 組の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// 中身が同じ(指紋が一致)
    Same,
    /// 名前が似ている(末尾の番号・v2・(1)・コピーを除くと同じ)
    Name,
    /// 形が近い(体積・外形の箱・面の数が 2% 以内で、名前は違う)
    Shape,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Same, Kind::Name, Kind::Shape];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Same => "中身が同じ",
            Kind::Name => "名前が似ている",
            Kind::Shape => "形が近い",
        }
    }
}

/// 同じ・似ているデザインの組。`members` は残す候補(版の多い物、同じなら新しい物)が先。
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub kind: Kind,
    pub members: Vec<PathBuf>,
}

/// 名前の飾り(末尾の番号・v2・(1)・コピー・「 - コピー」・「のコピー」)を除いた形(大小・全角半角は
/// そろえる)。
pub fn base_name(stem: &str) -> String {
    let mut s = fold(stem.trim());
    loop {
        let before = s.clone();
        for suffix in [
            " - コピー",
            "のこぴー",
            "こぴー",
            " - copy",
            "copy",
            "のコピー",
            "コピー",
        ] {
            let f = fold(suffix);
            if let Some(t) = s.strip_suffix(&f) {
                s = t.to_string();
            }
        }
        // 「(1)」「(2)」
        if s.ends_with(')')
            && let Some(k) = s.rfind('(')
            && s[k + 1..s.len() - 1].chars().all(|c| c.is_ascii_digit())
            && s.len() > k + 2
        {
            s.truncate(k);
        }
        // 末尾の番号と「v2」
        let t = s.trim_end_matches(|c: char| c.is_ascii_digit());
        if t.len() < s.len() {
            let t = t.strip_suffix('v').unwrap_or(t);
            s = t.to_string();
        }
        s = s
            .trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '_' | '-' | '.'))
            .to_string();
        if s == before {
            break;
        }
    }
    if s.is_empty() { fold(stem.trim()) } else { s }
}

/// 分類(ジャンル・作品・タグ)が 1 つも無いか。
pub fn unclassified(info: &ModelInfo) -> bool {
    info.genre.trim().is_empty() && info.project.trim().is_empty() && info.tags.is_empty()
}

/// 同じ・似ているデザインの組を探す(中身が同じ → 名前が似ている → 形が近い の順)。
pub fn groups(entries: &[Entry]) -> Vec<Group> {
    let by_path: HashMap<&Path, &Entry> = entries.iter().map(|e| (e.path.as_path(), e)).collect();
    let keep_first = |mut v: Vec<PathBuf>| {
        v.sort_by(|a, b| {
            let (a, b) = (by_path[a.as_path()], by_path[b.as_path()]);
            b.versions
                .cmp(&a.versions)
                .then(b.modified.cmp(&a.modified))
                .then(a.path.cmp(&b.path))
        });
        v
    };
    let mut out = Vec::new();
    let mut same: HashMap<&str, Vec<PathBuf>> = HashMap::new();
    let mut names: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for e in entries {
        same.entry(&e.fingerprint).or_default().push(e.path.clone());
        names
            .entry(base_name(&e.name))
            .or_default()
            .push(e.path.clone());
    }
    for v in same.into_values().filter(|v| v.len() > 1) {
        out.push(Group {
            kind: Kind::Same,
            members: keep_first(v),
        });
    }
    for v in names.into_values().filter(|v| v.len() > 1) {
        // 全部が同じ中身なら「中身が同じ」の組で足りる
        let first = &by_path[v[0].as_path()].fingerprint;
        if v.iter().all(|p| &by_path[p.as_path()].fingerprint == first) {
            continue;
        }
        out.push(Group {
            kind: Kind::Name,
            members: keep_first(v),
        });
    }
    // 形が近い: 体積の順に並べ、2% の窓の中だけ比べる(名前が似ている物・中身が同じ物どうしは除く)
    let mut shaped: Vec<(&Entry, Shape)> =
        entries.iter().filter_map(|e| Some((e, e.shape?))).collect();
    shaped.sort_by(|a, b| a.1.volume.total_cmp(&b.1.volume));
    let mut parent: Vec<usize> = (0..shaped.len()).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for i in 0..shaped.len() {
        for j in i + 1..shaped.len() {
            let (a, sa) = shaped[i];
            let (b, sb) = shaped[j];
            if sb.volume > sa.volume * (1.0 + SHAPE_TOL) + f64::EPSILON {
                break;
            }
            if sa.near(&sb, SHAPE_TOL)
                && a.fingerprint != b.fingerprint
                && base_name(&a.name) != base_name(&b.name)
            {
                let (ra, rb) = (root(&mut parent, i), root(&mut parent, j));
                parent[ra] = rb;
            }
        }
    }
    let mut sets: HashMap<usize, Vec<PathBuf>> = HashMap::new();
    for (i, (e, _)) in shaped.iter().enumerate() {
        let r = root(&mut parent, i);
        sets.entry(r).or_default().push(e.path.clone());
    }
    for v in sets.into_values().filter(|v| v.len() > 1) {
        out.push(Group {
            kind: Kind::Shape,
            members: keep_first(v),
        });
    }
    out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.members[0].cmp(&b.members[0])));
    out
}

/// 組の違いの表の行 1 つ(右の「この組の違い」)。
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub label: &'static str,
    /// デザインごとの値(並びは渡した順)
    pub values: Vec<String>,
    /// 値がそろっていないか(画面は目立たせる)
    pub differs: bool,
}

/// 組のデザインを並べて比べる表(場所・版の数・更新・大きさ・体積・外形の箱・面の数・分類)。
pub fn compare(entries: &[&Entry]) -> Vec<Row> {
    use crate::library::{bytes_label, date_label};
    let col =
        |f: &dyn Fn(&Entry) -> String| -> Vec<String> { entries.iter().map(|e| f(e)).collect() };
    let shape =
        |e: &Entry, f: &dyn Fn(&Shape) -> String| e.shape.as_ref().map(f).unwrap_or_default();
    let rows: Vec<(&'static str, Vec<String>)> = vec![
        (
            "場所",
            col(&|e| {
                e.path
                    .parent()
                    .and_then(Path::file_name)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default()
            }),
        ),
        ("版の数", col(&|e| e.versions.to_string())),
        (
            "更新",
            col(&|e| e.modified.map(date_label).unwrap_or_default()),
        ),
        ("大きさ", col(&|e| bytes_label(e.bytes))),
        (
            "体積",
            col(&|e| shape(e, &|s| format!("{:.0} mm³", s.volume))),
        ),
        (
            "外形の箱",
            col(&|e| {
                shape(e, &|s| {
                    format!(
                        "{} × {} × {} mm",
                        num(s.size[0]),
                        num(s.size[1]),
                        num(s.size[2])
                    )
                })
            }),
        ),
        ("面の数", col(&|e| shape(e, &|s| s.faces.to_string()))),
        ("ジャンル", col(&|e| e.info.genre.clone())),
        ("作品", col(&|e| e.info.project.clone())),
        ("タグ", col(&|e| e.info.tags.join(", "))),
    ];
    rows.into_iter()
        .map(|(label, values)| Row {
            label,
            differs: values.windows(2).any(|w| w[0] != w[1]),
            values,
        })
        .collect()
}

/// 長さを短く書く(整数ならそのまま、ほかは小数 1 桁)。
fn num(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format!("{r:.1}")
    }
}

/// 見つからないファイル 1 つ(最近の一覧に残っている物)。
#[derive(Debug, Clone, PartialEq)]
pub struct Missing {
    pub path: PathBuf,
    /// 移した記録から分かった今の場所(分からなければ無し)
    pub moved_to: Option<PathBuf>,
}

/// `paths` のうち見つからない物と、移した記録から分かる今の場所。
pub fn missing<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Vec<Missing> {
    paths
        .into_iter()
        .filter(|p| !p.is_file())
        .map(|p| Missing {
            path: p.to_path_buf(),
            moved_to: crate::organize::follow_moves(p),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Snapshot;

    #[test]
    fn names_lose_their_copy_marks_and_numbers() {
        for (a, b) in [
            ("天板", "天板"),
            ("天板 (1)", "天板"),
            ("天板(2)", "天板"),
            ("天板 - コピー", "天板"),
            ("天板のコピー", "天板"),
            ("天板_v2", "天板"),
            ("天板v10", "天板"),
            ("天板-3", "天板"),
            ("Tenban Copy", "tenban"),
            ("ＡＢＣ２", "abc"),
            ("123", "123"),
        ] {
            assert_eq!(base_name(a), b, "{a}");
        }
        assert_ne!(base_name("天板"), base_name("脚"));
    }

    #[test]
    fn groups_are_candidates_by_content_name_and_shape() {
        let base = std::env::temp_dir().join(format!("jc-tidy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("部品")).unwrap();
        let doc = jc_core::Document::new();
        let with_genre = |g: &str| {
            let mut d = jc_core::Document::new();
            let mut props = jc_core::Properties::default();
            props.set("genre", g);
            d.apply(jc_core::Transaction::new(
                "属性",
                vec![jc_core::Op::SetProperties {
                    properties: Box::new(props),
                }],
            ))
            .unwrap();
            d
        };
        let (other, third) = (with_genre("家具"), with_genre("棚"));
        let shape = |v: f64| Snapshot {
            png: None,
            shape: Some(Shape {
                volume: v,
                size: [100.0, 50.0, 18.0],
                faces: 6,
            }),
        };
        let a = base.join("天板.json");
        let b = base.join("部品").join("天板 (1).json");
        let c = base.join("脚.json");
        let d = base.join("棚板.json");
        let e = base.join("箱.json");
        crate::save_with(&a, &doc, &shape(90_000.0), "").unwrap();
        crate::save_with(&a, &other, &shape(90_000.0), "").unwrap();
        crate::save_with(&b, &doc, &Snapshot::default(), "").unwrap();
        crate::save_with(&c, &doc, &Snapshot::default(), "").unwrap();
        crate::save_with(&d, &third, &shape(91_000.0), "").unwrap();
        crate::save_with(&e, &doc, &shape(120_000.0), "").unwrap();
        let entries: Vec<Entry> = [&a, &b, &c, &d, &e]
            .iter()
            .map(|p| entry(p).unwrap())
            .collect();
        assert_eq!(entries[0].versions, 2);
        assert_eq!(
            entries[0].fingerprint,
            history::list(&a)[0].id,
            "保存した物は最新の版の指紋と同じ"
        );
        assert!(!unclassified(&entries[0].info));
        assert!(unclassified(&entries[1].info));
        let g = groups(&entries);
        let kinds: Vec<Kind> = g.iter().map(|g| g.kind).collect();
        assert_eq!(kinds, vec![Kind::Same, Kind::Name, Kind::Shape]);
        // 中身が同じ: 空の文書の 3 つ(天板 (1)・脚・箱)
        assert_eq!(g[0].members.len(), 3);
        // 名前が似ている: 天板と天板 (1)。版の多い天板が先(残す候補)
        assert_eq!(g[1].members, vec![a.clone(), b.clone()]);
        // 形が近い: 天板と棚板(1.1%)。箱は 33% 違うので入らない
        assert_eq!(g[2].members.len(), 2);
        assert!(g[2].members.contains(&d) && g[2].members.contains(&a));
        let table = compare(&[&entries[0], &entries[3]]);
        let row = |l: &str| table.iter().find(|r| r.label == l).unwrap().clone();
        assert!(row("版の数").differs && row("体積").differs && row("ジャンル").differs);
        assert!(!row("外形の箱").differs && !row("面の数").differs);
        assert_eq!(row("体積").values[0], "90000 mm³");
        // 見つからない物は、移した記録から今の場所が分かる
        let moved = crate::organize::move_folder(&base.join("部品"), &base, Some("小物")).unwrap();
        let m = missing([b.as_path(), a.as_path()]);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].moved_to, Some(moved.join("天板 (1).json")));
        let _ = std::fs::remove_dir_all(&base);
    }
}
