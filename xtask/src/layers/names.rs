//! 層の点検のうち、名前を調べる物: 他の層の名前(xtask/names.txt の持ち主の表)と、モジュールの
//! 名乗り(`module()`)が組み立てに 1 回ずつ並ぶこと。

use super::*;

/// 名前の持ち主の表(xtask/names.txt。jc-bundle の試験 `tests/ids.rs` が登録から書く)。
/// ID → 名乗った crate。
pub(super) fn names_table(root: &Path) -> BTreeMap<String, Vec<String>> {
    parse_names(&fs::read_to_string(root.join("xtask/names.txt")).unwrap_or_default())
}

pub(super) fn parse_names(text: &str) -> BTreeMap<String, Vec<String>> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_once('\t'))
        .map(|(id, by)| (id.to_string(), by.split(',').map(String::from).collect()))
        .collect()
}

/// 名前の頭(`cad.`)。頭の無い名前は `None`。
pub(super) fn head(id: &str) -> Option<&str> {
    let dot = id.find('.')?;
    let h = &id[..=dot];
    h[..dot]
        .bytes()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .then_some(h)
}

/// 他の層の名前を書いてはいけない層。コア・受け渡しの型・試験の部品・組み立ては区分や扱う層の
/// 名前を持つので外す。画面の拡張(jc-ui-*)は、依存する機能のモジュールの名前だけを書いてよい。
pub(super) fn must_not_name(layer: &str) -> &'static [&'static str] {
    match layer {
        "cad" => &["cam", "io", "analysis"],
        "cam" => &["cad", "analysis"],
        "analysis" => &["cad", "cam", "io"],
        "io" | "kernel" => &["cad", "cam", "analysis"],
        "ui" | "session" | "app" | "inspect" => &["cad", "cam", "io", "analysis"],
        _ => &[],
    }
}

/// 文字列の中の、表にある頭で始まる名前(位置と名前)。名前の途中(`f:solid.revolve@1` の
/// `solid.`)も拾うが、語の途中(`radio.`)は拾わない。
pub(super) fn names_in(s: &str, heads: &BTreeSet<&str>) -> Vec<String> {
    let b = s.as_bytes();
    let mut out = vec![];
    for h in heads {
        for (at, _) in s.match_indices(h) {
            if at > 0 && (b[at - 1].is_ascii_alphanumeric() || b[at - 1] == b'_') {
                continue;
            }
            let rest = &s[at + h.len()..];
            if !rest.starts_with(|c: char| c.is_ascii_lowercase()) {
                continue;
            }
            let tail: String = rest
                .chars()
                .take_while(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '@')
                })
                .collect();
            out.push(format!("{h}{}", tail.trim_end_matches('.')));
        }
    }
    out
}

/// 名前を名乗る crate(表に無ければ、同じ頭の名前を名乗る crate の全部)。
pub(super) fn owners_of<'t>(
    name: &str,
    table: &'t BTreeMap<String, Vec<String>>,
) -> (bool, Vec<&'t str>) {
    if let Some(by) = table.get(name) {
        return (true, by.iter().map(String::as_str).collect());
    }
    // `asm.position` は `asm.position@1` の種類
    let versioned = format!("{name}@");
    let mut by: Vec<&str> = table
        .iter()
        .filter(|(id, _)| id.starts_with(&versioned))
        .flat_map(|(_, by)| by.iter().map(String::as_str))
        .collect();
    if !by.is_empty() {
        by.sort();
        by.dedup();
        return (true, by);
    }
    let h = head(name).unwrap_or("");
    let mut by: Vec<&str> = table
        .iter()
        .filter(|(id, _)| head(id) == Some(h))
        .flat_map(|(_, by)| by.iter().map(String::as_str))
        .collect();
    by.sort();
    by.dedup();
    (false, by)
}

pub(super) fn foreign_names(
    root: &Path,
    g: &Graph,
    table: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    if table.is_empty() {
        return vec![
            "名前の持ち主の表 xtask/names.txt が無い。JC_BLESS=1 cargo test -p jc-bundle --test ids で作る"
                .into(),
        ];
    }
    let heads: BTreeSet<&str> = table.keys().filter_map(|id| head(id)).collect();
    let mut out = vec![];
    let mut used = BTreeSet::new();
    for &n in g.p.keys() {
        let l = g.layer(n);
        let ui_ext = is_ui_extension(n);
        let bad = must_not_name(l);
        if bad.is_empty() && !ui_ext {
            continue;
        }
        let deps: BTreeSet<&str> = g.deps(n, false).into_iter().chain([n]).collect();
        for (rel, _, src) in product_sources(root, g.dir(n)) {
            for lit in &src.literals {
                for name in names_in(&lit.text, &heads) {
                    let (exact, by) = owners_of(&name, table);
                    if by.is_empty() || by.contains(&n) {
                        continue;
                    }
                    let layers: BTreeSet<&str> = by.iter().map(|o| g.layer(o)).collect();
                    let wrong = if ui_ext {
                        // 依存しない機能のモジュールの名前(頭だけで分かる時は、その頭を持つ crate の
                        // どれにも依存していない時)
                        !by.iter().any(|o| deps.contains(o))
                            && layers
                                .iter()
                                .all(|x| matches!(*x, "cad" | "cam" | "io" | "analysis"))
                    } else {
                        layers.iter().all(|x| bad.contains(x))
                    };
                    if !wrong {
                        continue;
                    }
                    if NAME_KNOWN
                        .iter()
                        .any(|(a, f, s)| *a == n && *f == rel && *s == name)
                    {
                        used.insert((n.to_string(), rel.clone(), name));
                        continue;
                    }
                    let whose = if exact { "" } else { "と同じ頭の名前" };
                    out.push(format!(
                        "{n}({l})/{rel}:{} が {} {whose}の {name} を書いている\
                         (記述子・役割・受け渡しの型を通す)",
                        src.line_of(lit.at),
                        by.join("・"),
                    ));
                }
            }
        }
    }
    for (a, f, s) in NAME_KNOWN {
        if !used.contains(&(a.to_string(), f.to_string(), s.to_string())) {
            out.push(format!(
                "{a} の {f} にもう {s} は無い。xtask/src/layers.rs の NAME_KNOWN から消す"
            ));
        }
    }
    out
}

/// 種類を登録する crate(`pub fn register`)は `pub fn module()` で 1 つ名乗り、組み立ての中で
/// ちょうど 1 回だけ並ぶ。登録は名乗りを通すので、組み立ては `register` を直接呼ばない。
/// jc-solid は CAD の機能をまとめて登録し直す試験用なので数えない。
pub(super) fn modules_named_once(root: &Path, g: &Graph) -> Vec<String> {
    let mut out = vec![];
    let Some(assembly) = product_sources(root, g.dir("jc-bundle"))
        .into_iter()
        .find(|(rel, _, _)| rel == "src/lib.rs")
        .map(|(_, _, s)| s.code)
    else {
        return vec!["組み立て jc-bundle の src/lib.rs が読めない".into()];
    };
    for &n in g.p.keys() {
        if n == "jc-solid" {
            continue;
        }
        let Some(lib) = product_sources(root, g.dir(n))
            .into_iter()
            .find(|(rel, _, _)| rel == "src/lib.rs")
            .map(|(_, _, s)| s.code)
        else {
            continue;
        };
        let names = lib.contains("\npub fn module(");
        if lib.contains("\npub fn register(") && !names {
            out.push(format!(
                "{n} は種類を登録するのに module() で名乗っていない"
            ));
        }
        let ident = n.replace('-', "_");
        if assembly.contains(&format!("{ident}::register(")) {
            out.push(format!(
                "組み立てが {n} の register を直接呼んでいる(module() を並べる)"
            ));
        }
        if names {
            let times = assembly.matches(&format!("{ident}::module(")).count();
            if times != 1 {
                out.push(format!("{n} の名乗りが組み立ての中で {times} 回並んでいる"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_found_inside_strings_but_not_inside_words() {
        let heads: BTreeSet<&str> = ["cad.", "solid.", "io.", "asm."].into();
        assert_eq!(
            names_in("f:solid.revolve@1 と a:cad.revolve", &heads),
            ["cad.revolve", "solid.revolve@1"]
        );
        assert!(names_in("radio.png cad.", &heads).is_empty());
        assert_eq!(names_in("asm.position@1", &heads), ["asm.position@1"]);
        assert_eq!(head("cad.extrude@1"), Some("cad."));
        assert_eq!(head("debug"), None);
        assert_eq!(head("Foo.bar"), None);
    }

    #[test]
    fn owners_are_looked_up_by_the_whole_name_then_by_the_head() {
        let t = parse_names(
            "# 見出し\nasm.position@1\tjc-asm\ncad.extrude@1\tjc-cad-extrude\ncad.sketch@1\tjc-sketch\n",
        );
        assert_eq!(owners_of("asm.position@1", &t), (true, vec!["jc-asm"]));
        assert_eq!(owners_of("asm.position", &t), (true, vec!["jc-asm"]));
        assert_eq!(
            owners_of("cad.unknown", &t),
            (false, vec!["jc-cad-extrude", "jc-sketch"])
        );
        assert_eq!(owners_of("zzz.x", &t), (false, vec![]));
    }
}
