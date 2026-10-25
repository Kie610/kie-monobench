//! 資料の点検と生成(`cargo xtask docs`)。
//!
//! 機能ごとの記録(作った物・決めたこと)は、docs/plan.md の最後に 1 行ずつ足すのをやめ、
//! docs/機能/ の 1 機能 1 ファイルの断片に書く(並行の PR が同じ所に足して毎回衝突
//! したため)。一覧の表はコミットせず、このコマンドが target/docs/ に作る。docs/地図.md の crate の
//! 一覧だけは読む人が多いので、各 Cargo.toml の `description` と層から作ってコミットし、古ければ落とす。
//!
//! 点検(`verify` が毎回流す)で落とす物:
//! - 断片の名前と頭の欄(題・日付)と、節「作った物」が無い
//! - 断片に特許の点検の記録(頭の「特許の判定」「確定度」、節「特許の点検」)がある。特許の点検は
//!   公開する物に載せず、リポジトリの外の開発の記録に同じ名前のファイルで置く
//! - plan.md「決めたこと」に行を足した
//! - 機能の種類・操作・設定の名前(xtask/names.txt)か crate が増えたのに、新しい断片が無い
//! - docs に取り込みの衝突の印(`<<<<<<<`・`>>>>>>>`)が残る。表が見出しと区切りの行で始まらない
//! - docs/地図.md の crate の一覧が Cargo.toml と合わない
//! - 断片の「決定:」が plan.md・policy.md の「(決定: 名前)」に無い。同じ名前の決定が 2 つある
//!
//! Kie が作ると決めた事は plan.md・policy.md の行に「(決定: 名前)」を付け、作った断片の頭に
//! 「- 決定: 名前」を書く。まだ断片の無い決定は `cargo xtask docs` が一覧に出す(2026-10-23、
//! 決めたのに作業へ入っていない物が見つかった洗い出しの再発防止。O3 の書体のずれはこれで見える)。

use crate::Package;
use crate::layers::LAYERS;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// 断片の置き場所。
pub const DIR: &str = "docs/機能";
/// 地図.md の生成する所の始まりと終わり。
const MAP_BEGIN: &str =
    "<!-- crate の一覧: cargo xtask docs が各 Cargo.toml から作る。手で直さない -->";
const MAP_END: &str = "<!-- crate の一覧ここまで -->";
/// 行を足してはいけない節(ファイル、見出し)。
const FROZEN_SECTIONS: [(&str, &str); 1] = [("docs/plan.md", "## 決めたこと")];
/// 決定の印を探す資料。
const DECISION_FILES: [&str; 2] = ["docs/plan.md", "docs/policy.md"];
/// 決定の印の始まり(終わりは `)`)。
const DECISION_MARK: &str = "(決定: ";
/// 断片に置かない特許の点検の記録(頭の欄と節)。
const PATENT_FIELDS: [&str; 2] = ["特許の判定", "確定度"];
const PATENT_SECTION: &str = "特許の点検";
/// 他のソフトの更新から採った物の断片の頭の欄(.claude/rules/adopted.md)。どれか 1 つを書いたら全部要る。
const ADOPTED_FIELDS: [&str; 3] = ["元の項目", "意図", "仕分け"];
/// 「仕分け」の欄の頭に書く 4 つの分け方。
const ADOPTED_KINDS: [&str; 4] = ["A", "B", "C", "D"];

/// 断片 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    pub file: String,
    pub title: String,
    pub date: String,
    /// この断片で作った決定の名前(plan.md・policy.md の「(決定: 名前)」)
    pub decisions: Vec<String>,
    /// 節の名前と本文
    pub sections: Vec<(String, String)>,
}

/// 断片を読む。形の誤りは文で返す。
pub fn parse(file: &str, text: &str) -> Result<Fragment, Vec<String>> {
    let mut errs = vec![];
    let name_ok = file
        .strip_suffix(".md")
        .and_then(|s| {
            let (head, tag) = s.rsplit_once('-')?;
            let date = head.get(..10)?;
            let ok = is_date(date)
                && head.len() > 11
                && head.as_bytes()[10] == b'-'
                && tag.len() == 6
                && tag
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
            ok.then_some(())
        })
        .is_some();
    if !name_ok {
        errs.push(format!(
            "{file}: 名前は「日付-題-スレッドの末尾 6 字.md」(例 2026-10-21-トレース-vrkuhk.md)"
        ));
    }
    let mut lines = text.lines();
    let title = lines
        .next()
        .and_then(|l| l.strip_prefix("# "))
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if title.is_empty() {
        errs.push(format!("{file}: 1 行目は「# 題」"));
    }
    let mut date = String::new();
    let mut patent = false;
    let mut decisions = vec![];
    let mut adopted: BTreeMap<&str, String> = BTreeMap::new();
    let mut sections: Vec<(String, String)> = vec![];
    for l in lines {
        if let Some(h) = l.strip_prefix("## ") {
            sections.push((h.trim().to_string(), String::new()));
        } else if let Some((_, body)) = sections.last_mut() {
            body.push_str(l);
            body.push('\n');
        } else if let Some((k, v)) = l.strip_prefix("- ").and_then(|s| s.split_once(':')) {
            let v = v.trim().to_string();
            match k.trim() {
                "日付" => date = v,
                k if PATENT_FIELDS.contains(&k) => patent = true,
                "決定" => decisions = v.split('、').map(|d| d.trim().to_string()).collect(),
                k => {
                    if let Some(f) = ADOPTED_FIELDS.iter().find(|f| **f == k) {
                        adopted.insert(*f, v);
                    }
                }
            }
        }
    }
    for (_, b) in &mut sections {
        *b = b.trim().to_string();
    }
    if !is_date(&date) {
        errs.push(format!("{file}: 「- 日付: YYYY-MM-DD」が無い"));
    } else if name_ok && !file.starts_with(&date) {
        errs.push(format!("{file}: 名前の日付と「日付」({date})が違う"));
    }
    if !sections
        .iter()
        .any(|(n, b)| n == "作った物" && !b.is_empty())
    {
        errs.push(format!("{file}: 節「## 作った物」が無いか空"));
    }
    if patent || sections.iter().any(|(n, _)| n == PATENT_SECTION) {
        errs.push(format!(
            "{file}: 特許の点検の記録(「{}」の欄・節「## {PATENT_SECTION}」)は公開する物に載せない。\
             リポジトリの外の開発の記録に同じ名前で置き、\
             ここには避ける形の決まりだけを docs/避ける形.md に書く",
            PATENT_FIELDS.join("」「")
        ));
    }
    errs.extend(adopted_problems(file, &adopted));
    if errs.is_empty() {
        Ok(Fragment {
            file: file.to_string(),
            title,
            date,
            decisions,
            sections,
        })
    } else {
        Err(errs)
    }
}

/// 採った物の欄の誤り(点検: 採用分の仕分け)。作る前に意図を汲み取り 4 つに仕分けたことを、
/// 断片の頭の欄で残させる。欄を 1 つも書かない断片(自分で作った物)は見ない。
fn adopted_problems(file: &str, fields: &BTreeMap<&str, String>) -> Vec<String> {
    if fields.is_empty() {
        return vec![];
    }
    let mut errs = vec![];
    for f in ADOPTED_FIELDS {
        if fields.get(f).is_none_or(|v| v.is_empty()) {
            errs.push(format!(
                "{file}: 採った物の断片は「- {}:」を全部書く(「- {f}:」が無いか空。.claude/rules/adopted.md)",
                ADOPTED_FIELDS.join(":」「- ")
            ));
        }
    }
    if let Some(v) = fields.get("仕分け").filter(|v| !v.is_empty())
        && !ADOPTED_KINDS.iter().any(|k| v.starts_with(k))
    {
        errs.push(format!(
            "{file}: 「- 仕分け:」は {} のどれかで始める(土台あり・土台なし・作りが違う・前提が合わない)",
            ADOPTED_KINDS.join("・")
        ));
    }
    errs
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// docs/機能/ の断片の名前(README.md を除く)。
fn fragment_files(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(root.join(DIR))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md") && n != "README.md")
        .collect();
    v.sort();
    v
}

/// 断片を全部読む(読めない物は誤りに入れる)。
pub fn fragments(root: &Path) -> (Vec<Fragment>, Vec<String>) {
    let (mut ok, mut errs) = (vec![], vec![]);
    for f in fragment_files(root) {
        let text = fs::read_to_string(root.join(DIR).join(&f)).unwrap_or_default();
        match parse(&f, &text) {
            Ok(x) => ok.push(x),
            Err(e) => errs.extend(e),
        }
    }
    (ok, errs)
}

/// 決定の印(ファイル:行、名前)。同じ名前が 2 つあれば誤りに入れる。
pub fn decisions(root: &Path) -> (Vec<(String, String)>, Vec<String>) {
    let (mut out, mut errs) = (vec![], vec![]);
    for file in DECISION_FILES {
        let text = fs::read_to_string(root.join(file)).unwrap_or_default();
        for (name, line) in decision_marks(&text) {
            let at = format!("{file}:{line}");
            if let Some((first, _)) = out.iter().find(|(_, n)| *n == name) {
                errs.push(format!(
                    "{at}: 決定「{name}」は {first} にもある(名前を変える)"
                ));
            } else {
                out.push((at, name));
            }
        }
    }
    (out, errs)
}

/// 中身の「(決定: 名前)」の名前と行(1 から)。
fn decision_marks(text: &str) -> Vec<(String, usize)> {
    let mut out = vec![];
    for (i, l) in text.lines().enumerate() {
        let mut rest = l;
        while let Some(at) = rest.find(DECISION_MARK) {
            let r = &rest[at + DECISION_MARK.len()..];
            let Some(end) = r.find(')') else { break };
            out.push((r[..end].trim().to_string(), i + 1));
            rest = &r[end..];
        }
    }
    out
}

/// まだ断片の無い決定(ファイル:行 と名前)。
pub fn open_decisions(root: &Path) -> Vec<String> {
    let made: BTreeSet<String> = fragments(root)
        .0
        .into_iter()
        .flat_map(|f| f.decisions)
        .collect();
    decisions(root)
        .0
        .into_iter()
        .filter(|(_, n)| !made.contains(n))
        .map(|(at, n)| format!("{at}: {n}"))
        .collect()
}

/// 断片の「決定:」が印に無い物と、印の名前の重なり。
fn decision_problems(frags: &[Fragment], marks: &[(String, String)]) -> Vec<String> {
    let known: BTreeSet<&str> = marks.iter().map(|(_, n)| n.as_str()).collect();
    frags
        .iter()
        .flat_map(|f| f.decisions.iter().map(move |d| (f, d)))
        .filter(|(_, d)| !known.contains(d.as_str()))
        .map(|(f, d)| {
            format!(
                "{DIR}/{}: 決定「{d}」が {} の「{DECISION_MARK}…)」に無い",
                f.file,
                DECISION_FILES.join("・")
            )
        })
        .collect()
}

/// docs/地図.md の crate の一覧(層の順、層の中は名前の順)。
pub fn crate_list(packages: &[Package]) -> String {
    let mut out = vec![MAP_BEGIN.to_string()];
    for layer in LAYERS {
        let mut ps: Vec<&Package> = packages
            .iter()
            .filter(|p| p.layer.as_deref() == Some(layer))
            .collect();
        if ps.is_empty() {
            continue;
        }
        ps.sort_by(|a, b| a.name.cmp(&b.name));
        out.push(String::new());
        out.push(format!("層 {layer}:"));
        out.push(String::new());
        out.push("| crate | 役割 |".into());
        out.push("|---|---|".into());
        for p in ps {
            out.push(format!(
                "| {} | {} |",
                p.name,
                p.description
                    .as_deref()
                    .unwrap_or("(説明なし)")
                    .replace('|', "\\|")
            ));
        }
    }
    out.push(String::new());
    out.push(MAP_END.to_string());
    out.join("\n")
}

/// 地図.md の生成する所を今の一覧に替えた全文。印が無ければ `None`。
/// 改行はファイルに合わせる(Windows の git は取り出す時に CRLF にするので、LF の一覧と比べると
/// 中身が同じでも食い違う。2026-10-21 PC の取り込みで落ちた)。
fn map_with_list(map: &str, list: &str) -> Option<String> {
    let a = map.find(MAP_BEGIN)?;
    let b = map.find(MAP_END)? + MAP_END.len();
    let list = if map.contains("\r\n") {
        list.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        list.to_string()
    };
    (a < b).then(|| format!("{}{list}{}", &map[..a], &map[b..]))
}

/// 一覧の表を作る(target/docs/)。地図.md の crate の一覧も書き直す。
pub fn generate(root: &Path, packages: &[Package], out_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let (frags, errs) = fragments(root);
    if !errs.is_empty() {
        return Err(format!("断片の形が違います:\n  {}", errs.join("\n  ")));
    }
    fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let mut sorted = frags.clone();
    sorted.sort_by(|a, b| (&a.date, &a.file).cmp(&(&b.date, &b.file)));
    let mut made = vec![
        "# 作った物と決めたこと(docs/機能/ の断片から `cargo xtask docs` が作った)".to_string(),
    ];
    for f in &sorted {
        made.push(String::new());
        made.push(format!("## {} {}({})", f.date, f.title, f.file));
        for (name, body) in &f.sections {
            if !body.is_empty() {
                made.push(String::new());
                made.push(format!("### {name}"));
                made.push(String::new());
                made.push(body.clone());
            }
        }
    }
    let made_path = out_dir.join("作った物.md");
    fs::write(&made_path, made.join("\n") + "\n").map_err(|e| e.to_string())?;
    let map_path = root.join("docs/地図.md");
    let map = fs::read_to_string(&map_path).map_err(|e| e.to_string())?;
    let new = map_with_list(&map, &crate_list(packages))
        .ok_or_else(|| format!("docs/地図.md に印 {MAP_BEGIN} と {MAP_END} がありません"))?;
    if new != map {
        fs::write(&map_path, new).map_err(|e| e.to_string())?;
    }
    Ok(vec![made_path, map_path])
}

/// 点検に要る、分かれ目の側の中身。
pub struct Base<'a> {
    /// 分かれ目のファイルの中身(無ければ `None`)
    pub read: &'a dyn Fn(&str) -> Option<String>,
}

/// 資料の点検。誤りの文を返す。
pub fn check(root: &Path, packages: &[Package], base: &Base, changed: &[String]) -> Vec<String> {
    let (frags, mut out) = fragments(root);
    // 決定の行き先: 断片が指す決定が印にあり、印の名前が重ならない
    let (marks, dup) = decisions(root);
    out.extend(dup);
    out.extend(decision_problems(&frags, &marks));
    for (file, head) in FROZEN_SECTIONS {
        let Ok(text) = fs::read_to_string(root.join(file)) else {
            continue;
        };
        let n = section_lines(&text, head)
            .filter(|l| l.starts_with('|') || l.starts_with("- "))
            .count();
        if n > 0 {
            out.push(format!(
                "{file}「{head}」: 表や箇条の行が {n} 行ある。この節には足さず {DIR}/ の断片に書く"
            ));
        }
    }
    // 新しい機能の名前か crate が増えたのに、新しい断片が無い
    let added_names = names_added(root, base);
    let new_crates: Vec<&String> = changed
        .iter()
        .filter(|f| {
            (f.starts_with("crates/") || f.starts_with("apps/"))
                && f.matches('/').count() == 2
                && f.ends_with("/Cargo.toml")
                && (base.read)(f).is_none()
                && root.join(f).exists()
        })
        .collect();
    let new_fragment = changed.iter().any(|f| {
        f.strip_prefix(&format!("{DIR}/"))
            .is_some_and(|n| n.ends_with(".md") && n != "README.md")
            && (base.read)(f).is_none()
            && root.join(f).exists()
    });
    if (!added_names.is_empty() || !new_crates.is_empty()) && !new_fragment {
        let mut why: Vec<String> = added_names.iter().take(5).cloned().collect();
        why.extend(new_crates.iter().map(|c| c.to_string()));
        out.push(format!(
            "新しい機能の名前か crate を足したのに、{DIR}/ に新しい断片が無い(足した物: {})。\
             作った物を 1 ファイルに書く({DIR}/README.md)",
            why.join("、")
        ));
    }
    // docs の Markdown の形
    for f in md_files(&root.join("docs")) {
        let rel = f
            .strip_prefix(root)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(text) = fs::read_to_string(&f) else {
            continue;
        };
        out.extend(markdown_problems(&rel, &text));
    }
    // 地図.md の crate の一覧
    match fs::read_to_string(root.join("docs/地図.md")) {
        Ok(map) => match map_with_list(&map, &crate_list(packages)) {
            None => out.push(format!("docs/地図.md: 印 {MAP_BEGIN} と {MAP_END} が無い")),
            Some(new) if new != map => out.push(
                "docs/地図.md: crate の一覧が Cargo.toml と合わない(`cargo xtask docs` で作り直す)"
                    .into(),
            ),
            Some(_) => {}
        },
        Err(e) => out.push(format!("docs/地図.md を読めない: {e}")),
    }
    out
}

/// xtask/names.txt で分かれ目から増えた名前。
fn names_added(root: &Path, base: &Base) -> Vec<String> {
    let path = "xtask/names.txt";
    let Ok(now) = fs::read_to_string(root.join(path)) else {
        return vec![];
    };
    let old: BTreeSet<String> = (base.read)(path)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect();
    now.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#') && !old.contains(*l))
        .map(String::from)
        .collect()
}

/// 見出し `head` の節の行(次の同じ深さか浅い見出しまで)。
fn section_lines<'a>(text: &'a str, head: &'a str) -> impl Iterator<Item = &'a str> {
    let level = head.chars().take_while(|c| *c == '#').count();
    let mut inside = false;
    text.lines().filter(move |l| {
        if l.starts_with('#') {
            let lv = l.chars().take_while(|c| *c == '#').count();
            if l.trim_end() == head {
                inside = true;
                return false;
            }
            if inside && lv <= level {
                inside = false;
            }
        }
        inside
    })
}

/// 衝突の印と、見出しの無い表。
pub fn markdown_problems(file: &str, text: &str) -> Vec<String> {
    let mut out = vec![];
    let lines: Vec<&str> = text.lines().collect();
    let mut fence = false;
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if l.trim_start().starts_with("```") {
            fence = !fence;
            i += 1;
            continue;
        }
        if fence {
            i += 1;
            continue;
        }
        if l.starts_with("<<<<<<< ") || l.starts_with(">>>>>>> ") {
            out.push(format!("{file}:{}: 取り込みの衝突の印が残っている", i + 1));
        }
        if l.starts_with('|') {
            let start = i;
            while i < lines.len() && lines[i].starts_with('|') {
                i += 1;
            }
            let sep = lines.get(start + 1).is_some_and(|s| {
                s.starts_with('|')
                    && s.contains("---")
                    && s.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
            });
            if !sep {
                out.push(format!(
                    "{file}:{}: 表が見出しと区切りの行(|---|)で始まらない(表として描かれない)",
                    start + 1
                ));
            }
            continue;
        }
        i += 1;
    }
    out
}

fn md_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let Ok(rd) = fs::read_dir(dir) else {
        return out;
    };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            out.extend(md_files(&p));
        } else if p.extension().is_some_and(|e| e == "md") {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "# トレース\n\n- 日付: 2026-10-21\n\n## 作った物\n\n線をなぞる。\n";

    #[test]
    fn fragment_shape() {
        let f = parse("2026-10-21-トレース-vrkuhk.md", GOOD).unwrap();
        assert_eq!(f.title, "トレース");
        assert_eq!(f.sections, vec![("作った物".into(), "線をなぞる。".into())]);
        let errs = parse("トレース.md", &GOOD.replace("- 日付: 2026-10-21\n", "")).unwrap_err();
        assert_eq!(errs.len(), 2, "{errs:?}");
        let errs = parse("2026-10-20-トレース-vrkuhk.md", GOOD).unwrap_err();
        assert!(errs[0].contains("日付"), "{errs:?}");
        let errs = parse(
            "2026-10-21-トレース-vrkuhk.md",
            &GOOD.replace("線をなぞる。\n", ""),
        )
        .unwrap_err();
        assert!(errs[0].contains("作った物"), "{errs:?}");
    }

    #[test]
    fn patent_records_are_kept_out_of_fragments() {
        for text in [
            GOOD.replace("\n\n## 作った物", "\n- 特許の判定: 低\n\n## 作った物"),
            GOOD.replace("\n\n## 作った物", "\n- 確定度: A\n\n## 作った物"),
            format!("{GOOD}\n## 特許の点検\n\n古くからある。\n"),
        ] {
            let errs = parse("2026-10-21-トレース-vrkuhk.md", &text).unwrap_err();
            assert_eq!(errs.len(), 1, "{errs:?}");
            assert!(errs[0].contains("リポジトリの外"), "{errs:?}");
        }
    }

    #[test]
    fn adopted_fragments_keep_the_intent_and_the_kind() {
        let head =
            |fields: &str| GOOD.replace("\n\n## 作った物", &format!("\n{fields}\n## 作った物"));
        let ok = head(
            "- 元の項目: 2026-10-24 k1\n- 意図: 何も残らない交差で空のボディができる\n- 仕分け: C 試験だけ\n",
        );
        assert!(parse("2026-10-21-トレース-vrkuhk.md", &ok).is_ok());
        let errs = parse(
            "2026-10-21-トレース-vrkuhk.md",
            &head("- 元の項目: 2026-10-24 k1\n"),
        )
        .unwrap_err();
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert!(errs.iter().all(|e| e.contains("adopted.md")), "{errs:?}");
        let errs = parse(
            "2026-10-21-トレース-vrkuhk.md",
            &head("- 元の項目: 2026-10-24 k1\n- 意図: 困り事\n- 仕分け: 作る\n"),
        )
        .unwrap_err();
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("A・B・C・D"), "{errs:?}");
    }

    #[test]
    fn decisions_link_plan_lines_to_fragments() {
        let f = parse(
            "2026-10-21-トレース-vrkuhk.md",
            &GOOD.replace(
                "- 日付: 2026-10-21\n",
                "- 日付: 2026-10-21\n- 決定: 文字の書体、モデル管理\n",
            ),
        )
        .unwrap();
        assert_eq!(f.decisions, vec!["文字の書体", "モデル管理"]);
        let plan = "- 文字: 既定はゴシック体(Kie 2026-10-19)(決定: 文字の書体)\n\
                    - 版(決定: モデル管理)と(決定: 4 軸)\n";
        assert_eq!(
            decision_marks(plan),
            vec![
                ("文字の書体".to_string(), 1),
                ("モデル管理".to_string(), 2),
                ("4 軸".to_string(), 2)
            ]
        );
        // 印に無い決定を指す断片は落とす
        let marks = vec![("docs/plan.md:1".to_string(), "文字の書体".to_string())];
        let errs = decision_problems(&[f], &marks);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("モデル管理"), "{errs:?}");
    }

    #[test]
    fn decisions_without_fragment_are_listed() {
        let dir = std::env::temp_dir().join(format!("jc-docs-decisions-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(DIR)).unwrap();
        fs::write(
            dir.join("docs/plan.md"),
            "- 文字(決定: 文字の書体)\r\n- 版(決定: モデル管理)\r\n",
        )
        .unwrap();
        fs::write(dir.join("docs/policy.md"), "- 重ね(決定: 文字の書体)\n").unwrap();
        fs::write(
            dir.join(DIR).join("2026-10-21-トレース-vrkuhk.md"),
            GOOD.replace(
                "- 日付: 2026-10-21\n",
                "- 日付: 2026-10-21\n- 決定: モデル管理\n",
            ),
        )
        .unwrap();
        assert_eq!(open_decisions(&dir), vec!["docs/plan.md:1: 文字の書体"]);
        let errs = decisions(&dir).1;
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].starts_with("docs/policy.md:1"), "{errs:?}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn crlf_reads_the_same() {
        // PC(Windows)の取り出しは CRLF。どの点検も LF と同じ結果にする
        let crlf = |s: &str| s.replace('\n', "\r\n");
        let f = parse("2026-10-21-トレース-vrkuhk.md", &crlf(GOOD)).unwrap();
        assert_eq!(f, parse("2026-10-21-トレース-vrkuhk.md", GOOD).unwrap());
        assert!(markdown_problems("x.md", &crlf("| a |\n|---|\n| 1 |\n")).is_empty());
        let text = crlf("## 決めたこと\n\n- 足した行\n\n## 未決事項\n\n- 残す\n");
        assert_eq!(
            section_lines(&text, "## 決めたこと")
                .filter(|l| l.starts_with("- "))
                .count(),
            1
        );
    }

    #[test]
    fn tables_and_conflicts() {
        let ok = "| a | b |\n|---|---|\n| 1 | 2 |\n";
        assert!(markdown_problems("x.md", ok).is_empty());
        let broken = "段落\n| 2026 | 行だけ |\n| 2026 | 行だけ |\n";
        assert_eq!(markdown_problems("x.md", broken).len(), 1);
        let fenced = "```\n| 中 |\n```\n<<<<<<< HEAD\n";
        assert_eq!(markdown_problems("x.md", fenced).len(), 1);
    }

    #[test]
    fn frozen_section_lines() {
        let text = "## 決めたこと\n\n説明\n- 足した行\n\n## 未決事項\n\n- 残す\n";
        let v: Vec<&str> = section_lines(text, "## 決めたこと").collect();
        assert_eq!(v, vec!["", "説明", "- 足した行", ""]);
    }

    #[test]
    fn crate_list_follows_layers() {
        let pk = |n: &str, l: &str, d: &str| Package {
            name: n.into(),
            layer: Some(l.into()),
            description: Some(d.into()),
            ..Default::default()
        };
        let s = crate_list(&[
            pk("jc-ui", "ui", "画面"),
            pk("jc-core", "core", "核 | 文書"),
        ]);
        let core = s.find("| jc-core | 核 \\| 文書 |").unwrap();
        let ui = s.find("| jc-ui | 画面 |").unwrap();
        assert!(core < ui);
        let map = format!("前\n{MAP_BEGIN}\n古い\n{MAP_END}\n後\n");
        assert_eq!(map_with_list(&map, &s).unwrap(), format!("前\n{s}\n後\n"));
        // Windows の取り出し(CRLF)でも、同じ中身なら同じと見る
        let crlf = format!("前\n{s}\n後\n").replace('\n', "\r\n");
        assert_eq!(map_with_list(&crlf, &s).unwrap(), crlf);
    }
}
