//! 毎回読む骨格の資料(直下の AGENTS.md と .claude/rules/ の分野の決まり)の点検と生成。
//! 2026-10-22 Kie(提案「毎回読む骨格の資料」を推奨どおり)。
//!
//! 毎回読む資料は、古くなると全スレッドが毎回間違いを読むので、書いた物が実物とずれたら落とす:
//! - 層の表(生成区画)が `layers.rs` の表と合わない(`cargo xtask docs` で作り直す)
//! - コードの印(`…`)に書いたパス・crate・型や関数の名前・xtask の命令が無い
//! - 「policy.md「節」」の節が無い(見出しが同じか、同じ字に「(」が続く物)
//! - 決まりに付けた「(点検: 名前)」の名前が下の `CHECKS` に無い。`CHECKS` の点検がその所に無い
//! - 大きさの上限(AGENTS.md 12 KB、分野の決まり 1 つ 4 KB。毎回の呼び出しに入るため)を超えた
//! - 分野の決まりに `paths:` が無いか、どのファイルにも当たらない。CLAUDE.md が AGENTS.md を読まない
//!
//! 点検の無い決まりと、決まりの無い点検は落とさず数だけ出す(`cargo xtask docs` が一覧を出す)。
//! Windows の git は取り出す時に CRLF にするので、どの点検も LF に直してから比べる。

use crate::docs::markdown_problems;
use crate::layers::{DEV_EXTRA, DEV_KERNEL, LAYERS, allowed};
use crate::source::rust_files;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// 毎回読む資料。
pub const FILE: &str = "AGENTS.md";
/// 分野の決まりの置き場所(その分野のファイルを触った時だけ読まれる)。
const RULES: &str = ".claude/rules";
const MAX_AGENTS: usize = 12 * 1024;
const MAX_RULE: usize = 4 * 1024;
const TABLE_BEGIN: &str =
    "<!-- 層の表: cargo xtask docs が xtask/src/layers.rs から作る。手で直さない -->";
const TABLE_END: &str = "<!-- 層の表ここまで -->";

/// 決まりに付ける点検の名前と、その点検がある所(ファイルと、そこにあるはずの字)。
/// 点検を消したり名前を変えたりしたら、ここと AGENTS.md の「(点検: …)」を同じ PR で直す。
pub const CHECKS: &[(&str, &str, &str)] = &[
    ("層の向き", "xtask/src/layers.rs", "fn allowed("),
    (
        "許したフォルダ",
        "apps/kmb-mcp/src/tests.rs",
        "fn every_file_arg_of_every_action_stays_inside_the_allowed_folders(",
    ),
    ("他製品の名前", "xtask/src/products.rs", "pub fn check("),
    ("言葉の揺れ", "xtask/src/words.rs", "pub fn check("),
    (
        "名前の一意",
        "crates/kmb-bundle/tests/ids.rs",
        "fn action_and_setting_ids_and_roles_are_unique_and_targets_exist(",
    ),
    (
        "他の層の名前",
        "xtask/src/layers/names.rs",
        "fn foreign_names(",
    ),
    ("出し直し", "xtask/src/layers.rs", "fn reexports("),
    (
        "名乗り",
        "xtask/src/layers/names.rs",
        "fn modules_named_once(",
    ),
    (
        "戦略とカーネル",
        "xtask/src/layers/code.rs",
        "fn strategy_kernel_uses(",
    ),
    (
        "機械の名前",
        "xtask/src/layers/code.rs",
        "fn machine_names(",
    ),
    (
        "引数の直読み",
        "xtask/src/layers/code.rs",
        "fn raw_arg_reads(",
    ),
    (
        "直の書き出し",
        "xtask/src/layers/code.rs",
        "fn direct_output_writes(",
    ),
    (
        "数値の置き場",
        "xtask/src/layers/code.rs",
        "fn numeric_libraries(",
    ),
    (
        "新しい static",
        "xtask/src/layers/code.rs",
        "fn process_statics(",
    ),
    ("画面の Op", "xtask/src/layers/code.rs", "fn screen_ops("),
    (
        "画面の変更の組み立て",
        "xtask/src/layers/code.rs",
        "fn screen_edits(",
    ),
    (
        "画面の呼ばない口",
        "xtask/src/layers/code.rs",
        "fn session_only_calls(",
    ),
    (
        "前の機能を書き換えない",
        "xtask/src/layers/code.rs",
        "fn rewrites_on_insert(",
    ),
    (
        "文書の拡張子",
        "xtask/src/layers/code.rs",
        "fn document_extensions(",
    ),
    (
        "画面の再計算",
        "xtask/src/layers/code.rs",
        "fn screen_recomputes(",
    ),
    (
        "変えたら直す合図",
        "xtask/src/agents.rs",
        "pub fn change_hints(",
    ),
    (
        "Workspace の欄",
        "xtask/src/layers/code.rs",
        "fn workspace_fields(",
    ),
    (
        "右クリックの素のボタン",
        "xtask/src/layers/code.rs",
        "fn right_menu_buttons(",
    ),
    ("検査の錠", "xtask/src/main.rs", "fn verify_lock("),
    ("飛ばし", "xtask/src/skips.rs", "pub fn check("),
    ("消えたテスト", "xtask/src/skips.rs", "pub fn removed("),
    ("待ちの時間切れ", "xtask/src/skips.rs", "fn wait_loops("),
    ("資料", "xtask/src/docs.rs", "pub fn check("),
    ("使う人向けの資料", "xtask/src/prose.rs", "pub fn check("),
    ("ラベル", "xtask/src/labels.rs", "pub fn check("),
    (
        "記録の資料の経緯",
        "xtask/src/prose.rs",
        "pub fn check_records(",
    ),
    (
        "採用分の仕分け",
        "xtask/src/docs.rs",
        "fn adopted_problems(",
    ),
    (
        "決定の行き先",
        "xtask/src/docs.rs",
        "pub fn open_decisions(",
    ),
    ("骨格の資料", "xtask/src/agents.rs", "pub fn check("),
    ("画板の並び", "xtask/src/board.rs", "pub fn check("),
    ("コアの API", "xtask/src/api.rs", "pub fn diff("),
    ("ライセンス", "deny.toml", "[licenses]"),
    (
        "引数の宣言",
        "crates/kmb-core/src/args.rs",
        "fn check_args(",
    ),
    (
        "キーの割り当て",
        "crates/kmb-ui/src/keys_tests.rs",
        "fn i_measures_m_moves_and_s_opens_the_search_at_the_cursor(",
    ),
    (
        "選びの切替",
        "crates/kmb-ui/src/design/select_tests.rs",
        "fn picking_a_face_drops_the_feature_selection_so_delete_keeps_the_feature(",
    ),
];

/// 点検の結果。
#[derive(Debug, Default)]
pub struct Report {
    /// 落とす物
    pub problems: Vec<String>,
    /// 点検の名前が付いていない決まり(ファイル:行 と頭の字)
    pub unchecked: Vec<String>,
    /// どの決まりにも付いていない点検
    pub unused: Vec<String>,
}

/// LF に直した中身(無ければ `None`)。
fn read(root: &Path, file: &str) -> Option<String> {
    fs::read_to_string(root.join(file))
        .ok()
        .map(|s| s.replace("\r\n", "\n"))
}

/// 層の表(印を含む)。
pub fn layer_table() -> String {
    let mut out = vec![
        TABLE_BEGIN.to_string(),
        String::new(),
        "| 層 | 通常の依存に持ってよい層 |".into(),
        "|---|---|".into(),
    ];
    for l in LAYERS {
        let a = allowed(l);
        out.push(format!(
            "| {l} | {} |",
            if a.is_empty() {
                "(なし)".to_string()
            } else {
                a.join("・")
            }
        ));
    }
    out.push(String::new());
    out.push(format!(
        "テスト用の依存は、上に足して {} を使ってよい。テスト用にカーネルを直に使ってよいのは {} だけ。",
        DEV_EXTRA.join("・"),
        DEV_KERNEL.join("・")
    ));
    out.push(TABLE_END.to_string());
    out.join("\n")
}

/// `text`(LF)の生成区画を `table` に替えた全文。印が無ければ `None`。
fn with_table(text: &str, table: &str) -> Option<String> {
    let a = text.find(TABLE_BEGIN)?;
    let b = text.find(TABLE_END)? + TABLE_END.len();
    (a < b).then(|| format!("{}{table}{}", &text[..a], &text[b..]))
}

/// AGENTS.md の層の表を書き直す。書き直したら `true`。
pub fn generate(root: &Path) -> Result<bool, String> {
    let path = root.join(FILE);
    let raw = fs::read_to_string(&path).map_err(|e| format!("{FILE} を読めない: {e}"))?;
    let crlf = raw.contains("\r\n");
    let text = raw.replace("\r\n", "\n");
    let mut new = with_table(&text, &layer_table())
        .ok_or_else(|| format!("{FILE} に印 {TABLE_BEGIN} と {TABLE_END} がありません"))?;
    if crlf {
        new = new.replace('\n', "\r\n");
    }
    if new == raw {
        return Ok(false);
    }
    fs::write(&path, new).map_err(|e| e.to_string())?;
    Ok(true)
}

/// ソースから集めた名前。
#[derive(Default)]
struct Index {
    /// 定義された名前(関数・型・定数・マクロ・欄・列挙の値)
    defs: BTreeSet<String>,
    /// crate のフォルダの名前 → その crate で定義された名前
    by_crate: BTreeMap<String, BTreeSet<String>>,
    /// ソースに出てくる語(コード・文字列・コメント)
    words: BTreeSet<String>,
    /// xtask のソースの文字列(命令と旗)
    xtask: String,
    /// CLI のソースの文字列(`kmb` の命令)
    cli: String,
}

fn ident_at(s: &str) -> Option<&str> {
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    let id = &s[..end];
    (id.chars().next()?.is_ascii_alphabetic() || id.starts_with('_')).then_some(id)
}

/// 1 行の定義の名前(`pub fn x`・`struct X`・`macro_rules! x`・欄 `x:`・列挙の値 `X,` など)。
fn def_in(line: &str) -> Option<&str> {
    let mut l = line.trim_start();
    for v in ["pub(crate) ", "pub(super) ", "pub "] {
        if let Some(r) = l.strip_prefix(v) {
            l = r;
            break;
        }
    }
    for m in ["const fn ", "async fn ", "unsafe fn "] {
        if l.starts_with(m) {
            l = &l[m.len() - 3..];
        }
    }
    for k in [
        "fn ",
        "struct ",
        "enum ",
        "trait ",
        "type ",
        "const ",
        "static ",
        "mod ",
        "union ",
        "macro_rules! ",
    ] {
        if let Some(r) = l.strip_prefix(k) {
            return ident_at(r);
        }
    }
    let id = ident_at(l)?;
    let rest = &l[id.len()..];
    let field = rest.starts_with(':') && !rest.starts_with("::");
    let variant = id.starts_with(|c: char| c.is_ascii_uppercase())
        && (rest.is_empty()
            || rest.starts_with([',', '('])
            || rest.starts_with(" {")
            || rest.starts_with(" ="));
    (field || variant).then_some(id)
}

fn words_in(text: &str, out: &mut BTreeSet<String>) {
    for w in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if !w.is_empty() {
            out.insert(w.to_string());
        }
    }
}

impl Index {
    fn build(root: &Path) -> Index {
        let mut ix = Index::default();
        for top in ["crates", "apps", "xtask", "plugins"] {
            let dirs: Vec<_> = if top == "xtask" {
                vec![root.join("xtask")]
            } else {
                let mut v: Vec<_> = fs::read_dir(root.join(top))
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect();
                v.sort();
                v
            };
            for dir in dirs {
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut defs = BTreeSet::new();
                for sub in ["src", "tests", "build.rs"] {
                    let p = dir.join(sub);
                    let files = if p.is_file() { vec![p] } else { rust_files(&p) };
                    for f in files {
                        let Ok(text) = fs::read_to_string(&f) else {
                            continue;
                        };
                        for l in text.lines() {
                            if let Some(d) = def_in(l) {
                                defs.insert(d.to_string());
                            }
                        }
                        words_in(&text, &mut ix.words);
                        if top == "xtask" {
                            ix.xtask.push_str(&text);
                        }
                        if name == "kmb-cli" {
                            ix.cli.push_str(&text);
                        }
                    }
                }
                ix.defs.extend(defs.iter().cloned());
                ix.by_crate.insert(name, defs);
            }
        }
        ix
    }
}

fn camel(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_uppercase()) && s.chars().any(|c| c.is_ascii_lowercase())
}

/// 書いた物がパスか(空白が無く、`/` を含むか、よくある拡張子で終わる)。
fn is_path(s: &str) -> bool {
    const EXT: [&str; 10] = [
        ".md", ".rs", ".toml", ".ps1", ".sh", ".py", ".txt", ".json", ".wit", ".wgsl",
    ];
    !s.contains(char::is_whitespace)
        && !s.starts_with('.')
        && !s.contains("::")
        && (s.contains('/') || EXT.iter().any(|e| s.ends_with(e)))
}

/// `*` だけを使う字の型に `name` が合うか。
fn wildcard(pat: &str, name: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return pat == name;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !name.starts_with(first) || name.len() < first.len() + last.len() || !name.ends_with(last) {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for p in &parts[1..parts.len() - 1] {
        match rest.find(p) {
            Some(i) => rest = &rest[i + p.len()..],
            None => return false,
        }
    }
    true
}

/// パスの型(`*` と `**`)に当たるファイルかフォルダがあるか。target は見ない。
fn glob_exists(dir: &Path, segs: &[&str]) -> bool {
    let Some((seg, rest)) = segs.split_first() else {
        return dir.exists();
    };
    if seg.is_empty() {
        return glob_exists(dir, rest);
    }
    if *seg == "**" {
        if glob_exists(dir, rest) {
            return true;
        }
    } else if !seg.contains('*') {
        return glob_exists(&dir.join(seg), rest);
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return false;
    };
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n == "target" || n == ".git" {
            continue;
        }
        let p = e.path();
        if *seg == "**" {
            if p.is_dir() && glob_exists(&p, segs) {
                return true;
            }
        } else if wildcard(seg, &n) && glob_exists(&p, rest) {
            return true;
        }
    }
    false
}

/// コードの印 1 つの誤り(無ければ `None`)。
fn span_problem(root: &Path, ix: &Index, s: &str) -> Option<String> {
    if let Some(rest) = s.strip_prefix("cargo xtask ") {
        for w in rest.split_whitespace() {
            let w = w.split('=').next().unwrap_or(w);
            if w.starts_with('<') || w.starts_with('[') {
                continue;
            }
            if !ix.xtask.contains(&format!("\"{w}\"")) {
                return Some(format!("xtask に「{w}」が無い"));
            }
        }
        return None;
    }
    if s.starts_with("cargo ") {
        return None; // ほかの道具の命令は見ない
    }
    if let Some(rest) = s.strip_prefix("kmb ") {
        let w = rest.split_whitespace().next()?;
        return (!ix.cli.contains(&format!("\"{w}\""))).then(|| format!("kmb に命令「{w}」が無い"));
    }
    if s.starts_with("--") && !s.contains(' ') {
        return (!ix.xtask.contains(&format!("\"{s}\"")))
            .then(|| format!("xtask に旗「{s}」が無い"));
    }
    if is_path(s) {
        let segs: Vec<&str> = s.split('/').collect();
        return (!glob_exists(root, &segs)).then(|| format!("パス「{s}」が無い"));
    }
    if s.contains(char::is_whitespace) {
        // 文の印(`T<n> M6` など)は型の名前だけを見る
        let mut w = BTreeSet::new();
        words_in(s, &mut w);
        let missing: Vec<String> = w
            .into_iter()
            .filter(|t| camel(t) && !ix.defs.contains(t))
            .collect();
        return (!missing.is_empty()).then(|| format!("名前「{}」が無い", missing.join("・")));
    }
    if s.starts_with('.') || s.starts_with('@') {
        return None; // 拡張子や文書の横の置き場所
    }
    // crate の名前(`kmb-toolpath`)
    if s.contains('-')
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'*')
    {
        // 接頭辞(`kmb-`)は、その字で始まる crate があれば良い
        let pat = if s.ends_with('-') {
            format!("{s}*")
        } else {
            s.to_string()
        };
        let segs = |top: &'static str| vec![top, pat.as_str()];
        let ok = glob_exists(root, &segs("crates")) || glob_exists(root, &segs("apps"));
        return (!ok).then(|| format!("crate「{s}」が無い"));
    }
    // 名前の印(`Action::args`・`kmb_session::write_output`・`module()`・`ui_*`)
    let head = s.split('(').next().unwrap_or(s).trim_end_matches('!');
    let segs: Vec<&str> = head.split("::").collect();
    let ok_ident = |t: &str| {
        t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '*')
            && !t.is_empty()
    };
    if !segs.iter().all(|t| ok_ident(t)) {
        // `cx.arg`・`[package.metadata.kmb] layer` のような印は語だけを見る
        let mut w = BTreeSet::new();
        words_in(head, &mut w);
        let missing: Vec<String> = w
            .into_iter()
            .filter(|t| t.len() > 1 && !t.bytes().all(|c| c.is_ascii_digit()))
            .filter(|t| !ix.words.contains(t))
            .collect();
        return (!missing.is_empty()).then(|| format!("名前「{}」が無い", missing.join("・")));
    }
    let last = *segs.last()?;
    if segs.len() > 1 && segs[0].starts_with("kmb_") {
        let dir = segs[0].replace('_', "-");
        return match ix.by_crate.get(&dir) {
            None => Some(format!("crate「{dir}」が無い")),
            Some(d) if !d.contains(last) => Some(format!("{dir} に「{last}」が無い")),
            Some(_) => None,
        };
    }
    if segs.len() > 1 && segs[0].starts_with(|c: char| c.is_ascii_lowercase()) {
        return None; // 標準の物(`fs::write`)
    }
    let called = s.contains('(');
    for t in &segs {
        let need_def = camel(t) || (called && *t == last) || segs.len() > 1;
        let found = if let Some(p) = t.strip_suffix('*') {
            ix.words
                .iter()
                .any(|w| w.starts_with(p) && w.len() > p.len())
        } else if need_def {
            ix.defs.contains(*t)
        } else {
            ix.words.contains(*t)
        };
        if !found {
            return Some(format!("名前「{t}」が無い"));
        }
    }
    None
}

/// 文の中のコードの印(`…`)。囲み(```)の中は見ない。(行, 中身)。
fn spans(text: &str) -> Vec<(usize, String)> {
    let mut out = vec![];
    let mut fence = false;
    for (i, l) in text.lines().enumerate() {
        if l.trim_start().starts_with("```") {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        let parts: Vec<&str> = l.split('`').collect();
        for (k, p) in parts.iter().enumerate() {
            if k % 2 == 1 && k + 1 < parts.len() && !p.trim().is_empty() {
                out.push((i + 1, p.trim().to_string()));
            }
        }
    }
    out
}

/// 「x.md「節」「節」」の参照。(行, ファイル, 節)。
fn section_refs(text: &str) -> Vec<(usize, String, String)> {
    let mut out = vec![];
    for (i, l) in text.lines().enumerate() {
        let mut from = 0;
        while let Some(at) = l[from..].find(".md「") {
            let dot = from + at;
            let start = l[..dot]
                .rfind(|c: char| !(c.is_ascii_alphanumeric() || "/_-.".contains(c)))
                .map_or(0, |j| j + l[j..].chars().next().map_or(1, char::len_utf8));
            let file = format!("{}.md", &l[start..dot]);
            let mut rest = &l[dot + 3..];
            while let Some(r) = rest.strip_prefix('「') {
                let Some(end) = r.find('」') else { break };
                out.push((i + 1, file.clone(), r[..end].to_string()));
                rest = &r[end + '」'.len_utf8()..];
            }
            from = dot + 3;
        }
    }
    out
}

/// ファイルの見出しの字(`#` と空白を除く)。
fn headings(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.starts_with('#'))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .collect()
}

/// 「(点検: a・b)」の名前。(行, 名前)。
fn annotations(text: &str) -> Vec<(usize, String)> {
    let mut out = vec![];
    for (i, l) in text.lines().enumerate() {
        let mut rest = l;
        while let Some(at) = rest.find("(点検: ") {
            let r = &rest[at + "(点検: ".len()..];
            let Some(end) = r.find(')') else { break };
            for n in r[..end].split('・') {
                out.push((i + 1, n.trim().to_string()));
            }
            rest = &r[end..];
        }
    }
    out
}

/// 点検の名前を付けるべき決まり(2・3・4・6 節の箇条。小見出しの下は向かう先なので見ない)。
fn rules_without_check(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut on = false;
    for (i, l) in text.lines().enumerate() {
        if let Some(h) = l.strip_prefix("## ") {
            on = ["2.", "3.", "4.", "6."].iter().any(|n| h.starts_with(n));
            continue;
        }
        if l.starts_with("### ") {
            on = false;
        }
        let item = l.starts_with("- ")
            || l.split_once(". ")
                .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()));
        if on && item && !l.contains("(点検: ") {
            let head: String = l.chars().skip(2).take(24).collect();
            out.push(format!("{FILE}:{}: {head}…", i + 1));
        }
    }
    out
}

/// 分野の決まりの `paths:` の型。前書きが無ければ `None`。
fn rule_paths(text: &str) -> Option<Vec<String>> {
    let body = text.strip_prefix("---\n")?;
    let end = body.find("\n---")?;
    let front = &body[..end];
    let mut out = vec![];
    let mut inside = false;
    for l in front.lines() {
        if l.starts_with("paths:") {
            inside = true;
        } else if inside {
            if let Some(p) = l.trim().strip_prefix("- ") {
                out.push(p.trim().trim_matches('"').to_string());
            } else if !l.starts_with(' ') {
                inside = false;
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// 点検。
pub fn check(root: &Path) -> Report {
    check_with(root, CHECKS)
}

/// 点検の名前の表 `checks` で点検する。
fn check_with(root: &Path, checks: &[(&str, &str, &str)]) -> Report {
    let mut r = Report::default();
    let Some(agents) = read(root, FILE) else {
        r.problems.push(format!("{FILE} が無い"));
        return r;
    };
    if !read(root, "CLAUDE.md").is_some_and(|c| c.lines().any(|l| l.trim() == "@AGENTS.md")) {
        r.problems
            .push("CLAUDE.md が「@AGENTS.md」で AGENTS.md を読んでいない".into());
    }
    match with_table(&agents, &layer_table()) {
        None => r
            .problems
            .push(format!("{FILE}: 印 {TABLE_BEGIN} と {TABLE_END} が無い")),
        Some(new) if new != agents => r.problems.push(format!(
            "{FILE}: 層の表が xtask/src/layers.rs と合わない(`cargo xtask docs` で作り直す)"
        )),
        Some(_) => {}
    }
    let mut files = vec![(FILE.to_string(), agents.clone(), MAX_AGENTS)];
    let mut names: Vec<String> = fs::read_dir(root.join(RULES))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md"))
        .collect();
    names.sort();
    for n in names {
        let f = format!("{RULES}/{n}");
        let text = read(root, &f).unwrap_or_default();
        match rule_paths(&text) {
            None => r.problems.push(format!(
                "{f}: 前書きの `paths:` が無い(無いと毎回読まれる。毎回なら {FILE} に書く)"
            )),
            Some(ps) => {
                for p in ps {
                    let segs: Vec<&str> = p.split('/').collect();
                    if !glob_exists(root, &segs) {
                        r.problems
                            .push(format!("{f}: paths の「{p}」に当たるファイルが無い"));
                    }
                }
            }
        }
        files.push((f, text, MAX_RULE));
    }
    let ix = Index::build(root);
    let known: BTreeSet<&str> = checks.iter().map(|c| c.0).collect();
    let mut used = BTreeSet::new();
    for (f, text, max) in &files {
        if text.len() > *max {
            r.problems.push(format!(
                "{f}: {} バイトで上限 {max} を超えた(足すなら何かを削るか、正本へ移して節の名前で指す)",
                text.len()
            ));
        }
        r.problems.extend(markdown_problems(f, text));
        for (line, s) in spans(text) {
            if let Some(p) = span_problem(root, &ix, &s) {
                r.problems.push(format!("{f}:{line}: `{s}`: {p}"));
            }
        }
        for (line, doc, sec) in section_refs(text) {
            let path = if doc.contains('/') {
                doc.clone()
            } else {
                format!("docs/{doc}")
            };
            match read(root, &path) {
                None => r.problems.push(format!("{f}:{line}: {doc} が無い({path})")),
                Some(t) => {
                    let found = headings(&t)
                        .iter()
                        .any(|h| h == &sec || h.starts_with(&format!("{sec}(")));
                    if !found {
                        r.problems
                            .push(format!("{f}:{line}: {doc} に節「{sec}」が無い"));
                    }
                }
            }
        }
        for (line, n) in annotations(text) {
            if known.contains(n.as_str()) {
                used.insert(n);
            } else {
                r.problems.push(format!(
                    "{f}:{line}: 点検「{n}」が無い(xtask/src/agents.rs の CHECKS の名前を使う)"
                ));
            }
        }
    }
    for (name, file, needle) in checks {
        if !read(root, file).is_some_and(|t| t.contains(needle)) {
            r.problems.push(format!(
                "点検「{name}」が {file} に無い(「{needle}」が見つからない。CHECKS を直す)"
            ));
        }
        if !used.contains(*name) {
            r.unused.push((*name).to_string());
        }
    }
    r.unchecked = rules_without_check(&agents);
    r
}

/// 入出力の一覧(`cargo xtask io` が作る)。
pub const IO_FILE: &str = "docs/入出力.md";
/// AGENTS.md が決まりを書いている入出力の一覧の節(見出しの頭)。数の多い操作と設定は見ない
/// (足すたびに AGENTS.md を直す物ではない)。
const IO_WATCHED: [&str; 4] = [
    "## 役割の印",
    "## 文書の形式と版",
    "## 誤りの種類と CLI の終了の番号",
    "## MCP の道具",
];

/// 変えたら直す合図(仕組み 6): 層の表か、入出力の一覧の役割・文書の版・終了の番号・MCP の道具が
/// 分かれ目から変わったのに、AGENTS.md の本文(層の表の外)を触っていなければ、見直す所を返す。
/// 落とさず合図だけ出す(本文を直さなくてよい変更もあるため)。`old` は分かれ目の中身。
pub fn change_hints(root: &Path, old: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let now = |f: &str| read(root, f).unwrap_or_default();
    let was = |f: &str| old(f).map(|t| t.replace("\r\n", "\n"));
    hints(
        was(FILE).as_deref(),
        &now(FILE),
        was(IO_FILE).as_deref(),
        &now(IO_FILE),
    )
}

/// 生成区画と、それを除いた本文。
fn split_table(text: &str) -> (&str, String) {
    match (text.find(TABLE_BEGIN), text.find(TABLE_END)) {
        (Some(a), Some(b)) if a < b => (&text[a..b], format!("{}{}", &text[..a], &text[b..])),
        _ => ("", text.to_string()),
    }
}

/// `## 見出し` で始まる節(次の `## ` まで)。
fn section<'a>(text: &'a str, head: &str) -> &'a str {
    let Some(a) = text.find(head) else {
        return "";
    };
    let rest = &text[a..];
    let end = rest[head.len()..]
        .find("\n## ")
        .map_or(rest.len(), |i| i + head.len());
    &rest[..end]
}

fn hints(old_agents: Option<&str>, agents: &str, old_io: Option<&str>, io: &str) -> Vec<String> {
    let Some(old_agents) = old_agents else {
        return vec![];
    };
    let (old_table, old_body) = split_table(old_agents);
    let (table, body) = split_table(agents);
    if old_body != body {
        return vec![];
    }
    let mut out = vec![];
    if old_table != table {
        out.push(format!(
            "層の表が変わった。{FILE}「3. 構造」の本文(層の説明・向かう先)が今も合うか見直す"
        ));
    }
    if let Some(old_io) = old_io {
        for head in IO_WATCHED {
            if section(old_io, head) != section(io, head) {
                out.push(format!(
                    "{IO_FILE}「{}」が変わった。{FILE}「4. 入出力の約束」が今も合うか見直す",
                    head.trim_start_matches("## ")
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_hints_ask_to_review_the_skeleton_only_when_its_body_is_untouched() {
        let agents = format!("# 骨格\n本文\n{TABLE_BEGIN}\n| a |\n{TABLE_END}\n");
        let io = "# 一覧\n## 操作と引数(3 個)\nx\n## 役割の印(1 個)\n| a |\n## MCP の道具(2 個)\n| s |\n";
        // 何も変わらない・操作だけ増えた: 合図なし
        assert!(hints(Some(&agents), &agents, Some(io), io).is_empty());
        let more_actions = io.replace("x\n", "x\ny\n");
        assert!(hints(Some(&agents), &agents, Some(io), &more_actions).is_empty());
        // 役割と層の表が変わり、本文は同じ: 2 つ
        let roles = io.replace("| a |", "| a |\n| b |");
        let table = agents.replace("| a |", "| a |\n| b |");
        let h = hints(Some(&agents), &table, Some(io), &roles);
        assert_eq!(h.len(), 2, "{h:?}");
        assert!(h[1].contains("役割の印"), "{h:?}");
        // 本文も直した: 合図なし。分かれ目に資料が無い: 合図なし
        let both = table.replace("本文", "本文を直した");
        assert!(hints(Some(&agents), &both, Some(io), &roles).is_empty());
        assert!(hints(None, &table, None, &roles).is_empty());
    }
    use std::path::PathBuf;

    /// 試験用の小さな作業場(名前の付いた一時フォルダ)。
    fn tree(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("kmb-xtask-agents-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (f, text) in files {
            let p = root.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        }
        root
    }

    const LIB: &str = "pub struct Widget {\n    pub chrome: u8,\n}\npub enum Ink {\n    Travel,\n}\n\
        pub fn write_output() {}\n#[macro_export]\nmacro_rules! skip {\n    () => {};\n}\n\
        fn tools() -> &'static str { \"run_action\" }\n";
    const XTASK: &str = "fn main() { let _ = (\"verify\", \"--all\"); }\n";

    fn agents(body: &str) -> String {
        format!("# 骨格\n\n{}\n\n{body}\n", layer_table())
    }

    fn files<'a>(agents: &'a str, rule: &'a str) -> Vec<(&'a str, &'a str)> {
        vec![
            ("crates/kmb-thing/src/lib.rs", LIB),
            ("xtask/src/main.rs", XTASK),
            (
                "docs/policy.md",
                "# 方針\n\n## 決めたこと\n\n## 5. ログ(置き場所)\n",
            ),
            ("CLAUDE.md", "@AGENTS.md\n"),
            ("AGENTS.md", agents),
            (".claude/rules/thing.md", rule),
        ]
    }

    /// 試験の点検の名前の表(作業場の中にある物だけ)。
    const TEST_CHECKS: &[(&str, &str, &str)] = &[
        ("層の向き", "xtask/src/main.rs", "fn main"),
        ("飛ばし", "xtask/src/main.rs", "verify"),
    ];

    const RULE: &str =
        "---\npaths:\n  - \"crates/kmb-thing/**\"\n---\n# 決まり\n\n- `Widget` を使う。\n";

    #[test]
    fn written_names_must_exist() {
        // 修正後: 書いた物が全部ある
        let good = agents(
            "- `Widget`・`Ink::Travel`・`kmb_thing::write_output`\n\
             - `crates/kmb-thing/src/lib.rs`・`crates/kmb-*/src`・`kmb-thing`・`cargo xtask verify --all`\n\
             - `run_action`・`fs::write`・`.json`・`T<n> M6`・policy.md「決めたこと」「5. ログ」\n",
        );
        let root = tree("good", &files(&good, RULE));
        let r = check_with(&root, TEST_CHECKS);
        assert!(r.problems.is_empty(), "{:#?}", r.problems);
        // 修正前: 名前を変えた・消した・書き間違えた物は全部落ちる
        let bad = agents(
            "- `Widgit`・`Ink::Trav`・`kmb_thing::write_out`・`kmb_nothing::x`\n\
             - `crates/kmb-thing/src/nope.rs`・`crates/kmb-none*/src`・`kmb-other`・`cargo xtask verfy`\n\
             - `ui_*`・policy.md「決めたこと」「6. ログ」\n",
        );
        let root = tree("bad", &files(&bad, RULE));
        let r = check_with(&root, TEST_CHECKS);
        assert_eq!(r.problems.len(), 10, "{:#?}", r.problems);
        assert!(r.problems.iter().any(|p| p.contains("節「6. ログ」")));
    }

    #[test]
    fn table_size_paths_and_checks() {
        let body = "## 2. 哲学\n\n1. **小さく。** 守る(点検: 層の向き)。\n2. 調べる。\n\
            3. 守る(点検: 無い点検)。\n\n### 先\n\n- 先の話\n";
        let a = agents(body);
        let stale = a.replace("| core | core |", "| core | core・ui |");
        let big = format!("{RULE}{}", "あ".repeat(MAX_RULE));
        let no_front = "# 決まり\n";
        let root = tree("misc", &files(&stale, &big));
        let r = check_with(&root, TEST_CHECKS);
        assert!(
            r.problems.iter().any(|p| p.contains("層の表")),
            "{:#?}",
            r.problems
        );
        assert!(
            r.problems.iter().any(|p| p.contains("上限")),
            "{:#?}",
            r.problems
        );
        assert!(
            r.problems.iter().any(|p| p.contains("点検「無い点検」")),
            "{:#?}",
            r.problems
        );
        assert_eq!(r.unchecked.len(), 1, "{:?}", r.unchecked);
        assert!(r.unchecked[0].contains("調べる"));
        assert!(r.unused.contains(&"飛ばし".to_string()));
        assert!(!r.unused.contains(&"層の向き".to_string()));
        // 作り直せば表は合う。paths の無い決まりと当たらない paths は落ちる
        let root = tree("misc2", &files(&a, no_front));
        assert!(generate(&root).is_ok());
        let r = check_with(&root, TEST_CHECKS);
        assert!(r.problems.iter().all(|p| !p.contains("層の表")));
        assert!(
            r.problems.iter().any(|p| p.contains("paths")),
            "{:#?}",
            r.problems
        );
        let wrong = RULE.replace("kmb-thing/**", "kmb-gone/**");
        let r = check_with(&tree("misc3", &files(&a, &wrong)), TEST_CHECKS);
        assert!(
            r.problems.iter().any(|p| p.contains("kmb-gone")),
            "{:#?}",
            r.problems
        );
    }

    #[test]
    fn crlf_checks_the_same() {
        // PC(Windows)の取り出しは CRLF。LF と同じ結果にし、作り直しても CRLF のまま
        let a = agents("- `Widget` と policy.md「決めたこと」(点検: 層の向き)\n");
        let crlf = |s: &str| s.replace('\n', "\r\n");
        let (a2, r2) = (crlf(&a), crlf(RULE));
        let root = tree("crlf", &files(&a2, &r2));
        let r = check_with(&root, TEST_CHECKS);
        assert!(r.problems.is_empty(), "{:#?}", r.problems);
        assert_eq!(generate(&root), Ok(false));
        let stale = a2.replace("| core | core |", "| core | 古い |");
        fs::write(root.join(FILE), &stale).unwrap();
        assert_eq!(generate(&root), Ok(true));
        assert_eq!(fs::read_to_string(root.join(FILE)).unwrap(), a2);
    }

    #[test]
    fn small_parts() {
        assert!(wildcard("kmb-cad*", "kmb-cad-hole") && wildcard("kmb-*-x", "kmb-a-x"));
        assert!(!wildcard("kmb-cad*", "kmb-cam") && !wildcard("a*b", "ab-"));
        assert_eq!(def_in("    pub(crate) const fn go() {}"), Some("go"));
        assert_eq!(def_in("    Travel,"), Some("Travel"));
        assert_eq!(def_in("    pub ink3: Color32,"), Some("ink3"));
        assert_eq!(def_in("    let x: u8 = 1;"), None);
        assert_eq!(def_in("    std::fs::write(p, b)?;"), None);
        let refs = section_refs("(policy.md「目的」「構造」、docs/verify.md「5. ログ」)");
        assert_eq!(refs.len(), 3, "{refs:?}");
        assert_eq!(refs[2], (1, "docs/verify.md".into(), "5. ログ".into()));
        assert_eq!(
            annotations("- a(点検: 層の向き・名乗り)。b(点検: 資料)"),
            vec![
                (1, "層の向き".into()),
                (1, "名乗り".into()),
                (1, "資料".into())
            ]
        );
        assert_eq!(
            spans("`a` と `b c`\n```\n`x`\n```\n"),
            vec![(1, "a".into()), (1, "b c".into())]
        );
    }
}
