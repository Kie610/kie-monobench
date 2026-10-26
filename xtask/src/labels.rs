//! Issue と PR のラベルの点検。ラベルの定義は `.github/labels.yml` に 1 か所で持ち、
//! Issue のひな型(`.github/ISSUE_TEMPLATE/*.yml`)が付けるラベルがそこに無ければ落とす。
//! `cargo xtask labels` は、定義を GitHub に反映する `gh label create` のコマンドを出す
//! (反映は外への書き込みなので、維持する人が手で流す)。
//!
//! YAML の全部は読まず、この形だけを読む: 定義は `- name:`・`color:`・`description:` の行、
//! ひな型は `labels: [a, b]` の 1 行。

use std::fs;
use std::path::Path;

/// ラベルの定義のファイル(リポジトリの根から)。
pub const FILE: &str = ".github/labels.yml";

/// Issue のひな型の置き場(リポジトリの根から)。
const TEMPLATES: &str = ".github/ISSUE_TEMPLATE";

/// ラベル 1 つ。
#[derive(Debug, PartialEq)]
pub struct Label {
    pub name: String,
    pub color: String,
    pub description: String,
}

/// 定義の文を読む。読めない行は誤りの文にする。
pub fn parse(text: &str) -> Result<Vec<Label>, Vec<String>> {
    let mut out: Vec<Label> = vec![];
    let mut errs = vec![];
    for (i, line) in text.lines().enumerate() {
        let t = line.trim_end_matches('\r').trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(v) = t.strip_prefix("- name:") {
            out.push(Label {
                name: v.trim().to_string(),
                color: String::new(),
                description: String::new(),
            });
            continue;
        }
        let Some(last) = out.last_mut() else {
            errs.push(format!("{FILE}:{}: `- name:` より前に書いた行", i + 1));
            continue;
        };
        if let Some(v) = t.strip_prefix("color:") {
            last.color = v.trim().to_string();
        } else if let Some(v) = t.strip_prefix("description:") {
            last.description = v.trim().to_string();
        } else {
            errs.push(format!("{FILE}:{}: 知らない行 `{t}`", i + 1));
        }
    }
    for l in &out {
        let hex = l.color.len() == 6 && l.color.chars().all(|c| c.is_ascii_hexdigit());
        if !hex {
            errs.push(format!("{FILE}: {} の色が 16 進の 6 桁でない", l.name));
        }
        // GitHub のラベルの説明は 100 字まで
        if l.description.is_empty() || l.description.chars().count() > 100 {
            errs.push(format!("{FILE}: {} の説明が空か 100 字を超える", l.name));
        }
    }
    let mut names: Vec<&str> = out.iter().map(|l| l.name.as_str()).collect();
    names.sort_unstable();
    for w in names.windows(2) {
        if w[0] == w[1] {
            errs.push(format!("{FILE}: {} を 2 回定義した", w[0]));
        }
    }
    if errs.is_empty() { Ok(out) } else { Err(errs) }
}

/// ひな型の `labels: [a, b]` の名前。
pub fn template_labels(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("labels:"))
        .flat_map(|v| {
            v.trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// 点検の誤りの文(点検: ラベル)。
pub fn check(root: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(root.join(FILE)) else {
        return vec![format!("{FILE} が無い")];
    };
    let labels = match parse(&text) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let mut out = vec![];
    let Ok(dir) = fs::read_dir(root.join(TEMPLATES)) else {
        return out;
    };
    let mut files: Vec<_> = dir.flatten().map(|e| e.path()).collect();
    files.sort();
    for p in files {
        let is_yml = p.extension().is_some_and(|e| e == "yml" || e == "yaml");
        let Some(t) = is_yml.then(|| fs::read_to_string(&p).ok()).flatten() else {
            continue;
        };
        for name in template_labels(&t) {
            if !labels.iter().any(|l| l.name == name) {
                out.push(format!(
                    "{}: ラベル {name} が {FILE} に無い",
                    p.strip_prefix(root).unwrap_or(&p).display()
                ));
            }
        }
    }
    out
}

/// GitHub に反映するコマンド(既にあるラベルは `--force` で色と説明を合わせる)。
pub fn commands(labels: &[Label]) -> Vec<String> {
    labels
        .iter()
        .map(|l| {
            format!(
                "gh label create \"{}\" --color {} --description \"{}\" --force",
                l.name,
                l.color,
                l.description.replace('"', "\\\"")
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_labels_and_template_names() {
        let t = "# 注\r\n- name: bug\r\n  color: d73a4a\r\n  description: 動かない\r\n";
        let l = parse(t).unwrap();
        assert_eq!(l[0].name, "bug");
        assert_eq!(l[0].color, "d73a4a");
        assert!(commands(&l)[0].starts_with("gh label create \"bug\" --color d73a4a"));
        assert_eq!(
            template_labels("name: x\nlabels: [bug, \"good first issue\"]\n"),
            vec!["bug", "good first issue"]
        );
    }

    #[test]
    fn bad_definitions_are_listed() {
        let t = "- name: a\n  color: red\n  description: x\n- name: a\n  color: 000000\n";
        let e = parse(t).unwrap_err();
        assert_eq!(e.len(), 3, "{e:?}");
        assert!(parse("color: 000000\n").is_err());
    }

    #[test]
    fn the_repository_labels_cover_the_templates() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        assert_eq!(check(&root), Vec::<String>::new());
    }
}
