//! 層の点検のうち、ソースの書き方を字面で調べる物: 操作の引数の直読み、直の書き出し、戦略が
//! カーネルに触る所、汎用の加工の crate が機械を名指しする所、数値の解き方の置き場、プロセス全体の
//! static、画面の `Op` と再計算、画面の `Workspace` の欄。例外の表は親(layers.rs)に置く。

use super::*;

/// コード `code` の中で、操作の引数を `cx.args` から直に引いている所(`cx.args.get(`。間の空白と
/// 改行も見る)の位置。
pub(super) fn raw_reads(code: &str) -> Vec<usize> {
    code.match_indices(".args")
        .filter(|(at, _)| {
            let before = code[..*at].trim_end();
            let after = code[at + ".args".len()..].trim_start();
            let ident_end = !after.starts_with(|c: char| c.is_alphanumeric() || c == '_');
            let cx = before.ends_with("cx")
                && !before[..before.len() - 2].ends_with(|c: char| c.is_alphanumeric() || c == '_');
            cx && ident_end
                && after
                    .strip_prefix(".get")
                    .is_some_and(|r| r.trim_start().starts_with('('))
        })
        .map(|(at, _)| at)
        .collect()
}

/// コアの外で操作の引数を `cx.args.get(…)` で直に読んでいる所。`cx.arg(…)`・`cx.arg_f64(…)` で読む
/// (読んだ鍵を記録し、宣言 `Action::args` に無い鍵を読んだ操作を試験で落とすため)。
pub(super) fn raw_arg_reads(root: &Path, g: &Graph) -> Vec<String> {
    let mut out = vec![];
    for &n in g.p.keys() {
        if n == "kmb-core" {
            continue;
        }
        for (rel, _, src) in product_sources(root, g.dir(n)) {
            for at in raw_reads(&src.code) {
                out.push(format!(
                    "{n}/{rel}:{} が操作の引数を cx.args から直に読んでいる。cx.arg(\"鍵\") で読み、\
                     操作の宣言(Action::args)に鍵を書く",
                    src.line_of(at)
                ));
            }
        }
    }
    out
}

/// `std::fs::write(` か `File::create(` を直に呼ぶ所(行をまたぐ書き方も)。
pub(super) fn direct_writes(code: &str) -> Vec<usize> {
    let mut at: Vec<usize> = ["fs::write", "File::create"]
        .iter()
        .flat_map(|w| {
            code.match_indices(w).filter_map(move |(i, _)| {
                let before = &code[..i];
                let after = code[i + w.len()..].trim_start();
                let whole = !before.ends_with(|c: char| c.is_alphanumeric() || c == '_');
                (whole && after.starts_with('(')).then_some(i)
            })
        })
        .collect();
    at.sort_unstable();
    at
}

/// 画面とアプリの層で、書き出しを `std::fs::write` で直に書いている所。`kmb_session::write_output`
/// を通す(壊れない書き込みとログ。途中で落ちても半分のファイルを残さない)。
pub(super) fn direct_output_writes(root: &Path, g: &Graph) -> Vec<String> {
    let mut out = vec![];
    for &n in g.p.keys() {
        if !matches!(g.layer(n), "ui" | "app") {
            continue;
        }
        for (rel, _, src) in product_sources(root, g.dir(n)) {
            for at in direct_writes(&src.code) {
                out.push(format!(
                    "{n}/{rel}:{} がファイルを直に書いている。kmb_session::write_output(書き出し)か \
                     write_atomic(設定など)を通す",
                    src.line_of(at)
                ));
            }
        }
    }
    out
}

/// コード `code` の中で、カーネルに触る書き方(`.kernel()`・`ShapeHandle`・`dyn Kernel`)の位置。
pub(super) fn kernel_uses(code: &str) -> Vec<usize> {
    let mut at: Vec<usize> = [".kernel()", "ShapeHandle", "dyn Kernel"]
        .iter()
        .flat_map(|w| {
            code.match_indices(w).filter_map(move |(i, _)| {
                let after = &code[i + w.len()..];
                let whole = !after.starts_with(|c: char| c.is_alphanumeric() || c == '_');
                whole.then_some(i)
            })
        })
        .collect();
    at.sort_unstable();
    at
}

/// 加工の戦略(kmb-strategy-*)がカーネルに直に触っている所と、evaluate の中で経路を作っている所。
/// 戦略は解いた入力(kmb_cam_setup::StrategyInput)だけから経路を作る(WASM の戦略と同じ入口に
/// するため)。
pub(super) fn strategy_kernel_uses(
    root: &Path,
    g: &Graph,
    known: &[(&str, &str, &str)],
) -> Vec<String> {
    let mut out = vec![];
    let mut used = BTreeSet::new();
    for &n in g.p.keys().filter(|n| n.starts_with("kmb-strategy-")) {
        for (rel, _, src) in product_sources(root, g.dir(n)) {
            // 経路はどの戦略も plan で作り、ホストは finish_plan で出力にする(evaluate で経路を作らない)
            for w in [".finish(", ".toolpath(ctx"] {
                for (at, _) in src.code.match_indices(w) {
                    out.push(format!(
                        "{n}/{rel}:{} が evaluate の中で経路を作っている(`{w}`)。経路は plan(&StrategyInput) で作り、\
                         ホストは Prepared::finish_plan で出力にする",
                        src.line_of(at)
                    ));
                }
            }
            for at in kernel_uses(&src.code) {
                if known.iter().any(|(k, f, _)| *k == n && *f == rel) {
                    used.insert((n.to_string(), rel.clone()));
                    continue;
                }
                out.push(format!(
                    "{n}/{rel}:{} が戦略の中でカーネルに触っている。ホスト(evaluate)で解いて \
                     StrategyInput の sources に入れる",
                    src.line_of(at)
                ));
            }
        }
    }
    for (k, f, _) in known {
        if !used.contains(&(k.to_string(), f.to_string())) {
            out.push(format!(
                "{k}/{f} はもうカーネルに触っていない。xtask/src/layers.rs の STRATEGY_KERNEL_KNOWN から消す"
            ));
        }
    }
    out
}

/// 機械を名指ししてはいけない汎用の加工の crate か(機械の値とポストは機械の crate と組み立てが持つ)。
pub(super) fn is_generic_cam(n: &str) -> bool {
    n == "kmb-post"
        || n == "kmb-sim"
        || n.starts_with("kmb-cam-")
        || n.starts_with("kmb-strategy-")
        || n.starts_with("kmb-toolpath")
}

/// コードと文字列の中の、機械の名前(大小を無視)の位置。コメントと説明の文の中の例は数えない
/// ため、文字列は機械の ID の形(`carvera-air` のように名前だけの文字列)だけを見る。
pub(super) fn machine_words(src: &Source) -> Vec<usize> {
    const WORDS: [&str; 2] = ["carvera", "makera"];
    let code = src.code.to_ascii_lowercase();
    let mut at: Vec<usize> = WORDS
        .iter()
        .flat_map(|w| code.match_indices(w).map(|(i, _)| i).collect::<Vec<_>>())
        .collect();
    at.extend(src.literals.iter().filter_map(|l| {
        let v = l.text.to_ascii_lowercase();
        let id = !v.is_empty()
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        (id && WORDS.iter().any(|w| v.contains(w))).then_some(l.at)
    }));
    at.sort_unstable();
    at
}

/// Cargo.toml の `[dependencies]` に `name` があるか(テスト用・ビルド用の依存は見ない)。
pub(super) fn normal_dependency(toml: &str, name: &str) -> bool {
    let mut inside = false;
    for line in toml.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[dependencies]";
        } else if inside
            && line
                .split(['=', '.', ' '])
                .next()
                .is_some_and(|k| k.trim_matches('"') == name)
        {
            return true;
        }
    }
    false
}

/// 数値の解き方の置き場の外で nalgebra に依存する所。
pub(super) fn numeric_libraries(root: &Path, g: &Graph) -> Vec<String> {
    g.p.keys()
        .filter(|n| !NUMERIC_OWNERS.contains(n))
        .filter(|n| {
            std::fs::read_to_string(root.join(g.dir(n)).join("Cargo.toml"))
                .is_ok_and(|t| normal_dependency(&t, "nalgebra"))
        })
        .map(|n| {
            format!(
                "{n} が nalgebra に依存している。行列を使う数値の解き方は {} に置き、そこの関数を使う",
                NUMERIC_OWNERS.join("・")
            )
        })
        .collect()
}

/// 書き換えられる static(`static mut`、または型に Mutex・RwLock・OnceLock・Lazy・Atomic・Cell など
/// を持つ物。`thread_local!` の中も)の位置と名前。
pub(super) fn mutable_statics(code: &str) -> Vec<(usize, String)> {
    const INTERIOR: [&str; 8] = [
        "Mutex", "RwLock", "OnceLock", "OnceCell", "Lazy", "Atomic", "RefCell", "Cell<",
    ];
    let mut out = vec![];
    for (i, _) in code.match_indices("static ") {
        let before = &code[..i];
        if before.ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '\'') {
            continue;
        }
        let rest = code[i + "static ".len()..].trim_start();
        let (mutable, rest) = match rest.strip_prefix("mut ") {
            Some(r) => (true, r.trim_start()),
            None => (false, rest),
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let Some(ty) = rest[name.len()..].trim_start().strip_prefix(':') else {
            continue;
        };
        let ty = &ty[..ty.find(['=', ';']).unwrap_or(ty.len())];
        if !name.is_empty() && (mutable || INTERIOR.iter().any(|w| ty.contains(w))) {
            out.push((i, name));
        }
    }
    out
}

pub(super) fn process_statics(
    root: &Path,
    g: &Graph,
    known: &[(&str, &str, &str, &str)],
) -> Vec<String> {
    let mut out = vec![];
    let mut used = BTreeSet::new();
    for &n in
        g.p.keys()
            .filter(|n| NO_STATIC_LAYERS.contains(&g.layer(n)))
    {
        for (rel, _, src) in product_sources(root, g.dir(n)) {
            for (at, name) in mutable_statics(&src.code) {
                if known
                    .iter()
                    .any(|(k, f, s, _)| *k == n && *f == rel && *s == name)
                {
                    used.insert((n.to_string(), rel.clone(), name));
                    continue;
                }
                out.push(format!(
                    "{n}/{rel}:{} がプロセス全体の書き換えられる状態 {name} を置いている。設定は拡張の中\
                     (Extension::settings・set_settings)に、控えは文書かモジュールの持ち物に置く",
                    src.line_of(at)
                ));
            }
        }
    }
    for (k, f, s, _) in known {
        if !used.contains(&(k.to_string(), f.to_string(), s.to_string())) {
            out.push(format!(
                "{k}/{f} の {s} はもう無い。xtask/src/layers.rs の STATIC_KNOWN から消す"
            ));
        }
    }
    out
}

/// `Op::` の位置(`ToolOp::` などの別の名前は数えない)。
pub(super) fn op_uses(code: &str) -> Vec<usize> {
    code.match_indices("Op::")
        .filter(|(i, _)| !code[..*i].ends_with(|c: char| c.is_alphanumeric() || c == '_'))
        .map(|(i, _)| i)
        .collect()
}

pub(super) fn screen_ops(root: &Path, g: &Graph, known: &[(&str, &str, usize)]) -> Vec<String> {
    let crates: Vec<&str> =
        g.p.keys()
            .copied()
            .filter(|n| matches!(g.layer(n), "ui" | "app"))
            .collect();
    screen_counts(
        root,
        g,
        &crates,
        known,
        op_uses,
        "取り消しの変更(Op::)",
        "SCREEN_OP_KNOWN",
    )
}

/// 画面の crate(画面の層と、それを組むアプリ。CLI・MCP は入らない)。
fn screen_crates<'a>(g: &Graph<'a>) -> Vec<&'a str> {
    g.p.keys()
        .copied()
        .filter(|n| g.layer(n) == "ui" || *n == "kmb-app")
        .collect()
}

/// `edit::名前(`(kmb-session の変更の組み立て)と `Transaction::new(` の位置。
pub(super) fn edit_builds(code: &str) -> Vec<usize> {
    let mut at: Vec<usize> = code
        .match_indices("edit::")
        .filter(|(i, _)| !code[..*i].ends_with(|c: char| c.is_alphanumeric() || c == '_'))
        .filter(|(i, _)| {
            let rest = &code[i + "edit::".len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
                .collect();
            !name.is_empty() && rest[name.len()..].starts_with('(')
        })
        .map(|(i, _)| i)
        .collect();
    at.extend(calls_of(code, "Transaction::new"));
    at.sort_unstable();
    at
}

/// 画面とアプリの層で、文書の変更を kmb-session の edit でその場で組む所(8-1)。組んだ変更は CLI・MCP
/// から呼べないので、新しい操作は `Headless` の口に置き、画面もそれを呼ぶ。今ある所は減らすだけ。
pub(super) fn screen_edits(root: &Path, g: &Graph, known: &[(&str, &str, usize)]) -> Vec<String> {
    let crates = screen_crates(g);
    screen_counts(
        root,
        g,
        &crates,
        known,
        edit_builds,
        "文書の変更(edit::)",
        "SCREEN_EDIT_KNOWN",
    )
}

fn screen_counts(
    root: &Path,
    g: &Graph,
    crates: &[&str],
    known: &[(&str, &str, usize)],
    count: fn(&str) -> Vec<usize>,
    what: &str,
    table: &str,
) -> Vec<String> {
    let mut out = vec![];
    let mut seen = BTreeSet::new();
    for &n in crates {
        for (rel, _, src) in product_sources(root, g.dir(n)) {
            let at = count(&src.code);
            let allowed = known
                .iter()
                .find(|(k, f, _)| *k == n && *f == rel)
                .map_or(0, |e| e.2);
            if !at.is_empty() || allowed > 0 {
                seen.insert((n.to_string(), rel.clone()));
            }
            if at.len() > allowed {
                let lines: Vec<String> = at.iter().map(|&a| src.line_of(a).to_string()).collect();
                out.push(format!(
                    "{n}/{rel} が{what}をその場で組んでいる({} 個、許すのは {allowed} 個。\
                     行 {})。kmb_session の Headless の口を通す(CLI・MCP からも同じ操作ができる)",
                    at.len(),
                    lines.join("・")
                ));
            } else if at.len() < allowed {
                out.push(format!(
                    "{n}/{rel} の{what}が {} 個に減った。xtask/src/layers.rs の {table} を直す",
                    at.len()
                ));
            }
        }
    }
    for (k, f, _) in known {
        if !seen.contains(&(k.to_string(), f.to_string())) {
            out.push(format!(
                "{k}/{f} はもう無い。xtask/src/layers.rs の {table} から消す"
            ));
        }
    }
    out
}

/// `Headless` の文書を変える口(`pub fn 名前(&mut self`)のうち、画面とアプリの層が呼ばない物(8-1 の
/// 逆向き: CLI・MCP からしかできない操作)。`known` に無い物が増えたら落とし、画面が呼ぶように
/// なった物は `known` から消させる。
pub(super) fn session_only_calls(root: &Path, g: &Graph, known: &[&str]) -> Vec<String> {
    let dir = root.join("crates/kmb-session/src");
    let mut files = vec![dir.join("headless.rs")];
    files.extend(rust_files(&dir.join("headless")));
    let mut names = BTreeSet::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        for (i, _) in text.match_indices("pub fn ") {
            let rest = &text[i + "pub fn ".len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let head = &rest[name.len()..];
            let args = head.find('(').map(|p| &head[p + 1..]).unwrap_or("");
            if args.trim_start().starts_with("&mut self") {
                names.insert(name);
            }
        }
    }
    // 改行で切れた呼び出し(`self.cur\n.work\n.name(`)も見つけるよう、空白を除いて探す
    let screen: Vec<String> = screen_crates(g)
        .into_iter()
        .flat_map(|n| product_sources(root, g.dir(n)))
        .map(|(_, _, src)| src.code.split_whitespace().collect())
        .collect();
    let called = |name: &str| {
        let pat = format!("work.{name}(");
        screen.iter().any(|c| c.contains(&pat))
    };
    let mut out = vec![];
    for name in &names {
        let only = !called(name);
        let listed = known.contains(&name.as_str());
        if only && !listed {
            out.push(format!(
                "Headless::{name} を画面が呼ばない(CLI・MCP だけの道になる)。画面の同じ操作も\
                 この口を呼ぶか、画面に同じ操作を置く"
            ));
        } else if !only && listed {
            out.push(format!(
                "Headless::{name} を画面も呼ぶようになった。xtask/src/layers.rs の SESSION_ONLY_KNOWN から消す"
            ));
        }
    }
    for k in known {
        if !names.contains(*k) {
            out.push(format!(
                "Headless::{k} はもう無い。xtask/src/layers.rs の SESSION_ONLY_KNOWN から消す"
            ));
        }
    }
    out
}

/// 再計算を自分で回す・形の控えを直に扱うコアの口。画面とアプリの層は呼ばず、kmb-session の
/// `Headless`(start・poll・wait_recompute・recompute)を通す(CLI・MCP と同じ道で計算し、裏の
/// 再計算の取り消しと控えの受け渡しを 1 か所にする)。
pub(super) const RECOMPUTE_CALLS: &[&str] = &[
    "recompute_watched",
    "recompute_detached",
    "take_cache",
    "hold_shapes",
];

/// `name(` の位置(`my_take_cache(` などの別の名前は数えない)。
pub(super) fn calls_of(code: &str, name: &str) -> Vec<usize> {
    let pat = format!("{name}(");
    code.match_indices(&pat)
        .filter(|(i, _)| !code[..*i].ends_with(|c: char| c.is_alphanumeric() || c == '_'))
        .map(|(i, _)| i)
        .collect()
}

pub(super) fn screen_recomputes(root: &Path, g: &Graph) -> Vec<String> {
    let mut out = vec![];
    for &n in g.p.keys().filter(|n| matches!(g.layer(n), "ui" | "app")) {
        for (rel, _, src) in product_sources(root, g.dir(n)) {
            for name in RECOMPUTE_CALLS {
                for at in calls_of(&src.code, name) {
                    out.push(format!(
                        "{n}/{rel}:{} が {name} を直に呼んでいる。画面とアプリの再計算は kmb_session の \
                         Headless(start・poll・wait_recompute・recompute)を通す",
                        src.line_of(at)
                    ));
                }
            }
        }
    }
    out
}

/// struct `head`(`pub struct Workspace` など)の最上段の欄の名前。
pub(super) fn struct_fields(code: &str, head: &str) -> Option<Vec<String>> {
    let start = code.find(head)?;
    let body = &code[start..];
    let open = body.find('{')?;
    let mut depth = 0;
    let mut names = vec![];
    let mut field_start = open + 1;
    let mut prev = ' ';
    for (i, c) in body[open..].char_indices() {
        let at = open + i;
        let arrow = c == '>' && prev == '-';
        prev = c;
        match c {
            '{' | '(' | '[' | '<' => depth += 1,
            '}' | ')' | ']' | '>' if depth > 0 && !arrow => {
                depth -= 1;
                if depth == 0 {
                    // 最後の欄はカンマが無くてもよい
                    names.extend(field_name(&body[field_start..at]));
                    break;
                }
            }
            ',' if depth == 1 => {
                names.extend(field_name(&body[field_start..at]));
                field_start = at + 1;
            }
            _ => {}
        }
        if c == '{' && depth == 1 {
            field_start = at + 1;
        }
    }
    Some(names)
}

/// 欄 1 つ分の字(属性・`pub` を含む)から名前。
pub(super) fn field_name(text: &str) -> Option<String> {
    let mut t = text.trim();
    while t.starts_with("#[") {
        t = t[t.find(']')? + 1..].trim_start();
    }
    for p in ["pub(crate) ", "pub(super) ", "pub "] {
        t = t.strip_prefix(p).unwrap_or(t);
    }
    let (name, _) = t.split_once(':')?;
    let name = name.trim();
    (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
        .then(|| name.to_string())
}

pub(super) fn workspace_fields(root: &Path, known: usize) -> Vec<String> {
    let Ok(text) = fs::read_to_string(root.join(WORKSPACE_FILE)) else {
        return vec![format!("{WORKSPACE_FILE} を読めない")];
    };
    let src = Source::new(&text);
    let Some(names) = struct_fields(&src.code, "pub struct Workspace") else {
        return vec![format!("{WORKSPACE_FILE} に pub struct Workspace が無い")];
    };
    let mut out = match fs::read_to_string(root.join(DOC_STATE_FILE)) {
        Ok(t) => {
            let d = Source::new(&t);
            d.code
                .match_indices("egui")
                .filter(|(i, _)| {
                    !d.code[..*i].ends_with(|c: char| c.is_alphanumeric() || c == '_')
                })
                .map(|(i, _)| {
                    format!(
                        "{DOC_STATE_FILE}:{} が egui を使っている。文書ごとの状態には画面の型を置かない(位置は \
                         [f32; 2] などにするか、画面の側の状態に置く)",
                        d.line_of(i)
                    )
                })
                .collect()
        }
        Err(_) => vec![format!("{DOC_STATE_FILE} を読めない")],
    };
    out.extend(match names.len().cmp(&known) {
        std::cmp::Ordering::Greater => vec![format!(
            "画面の Workspace の欄が {} 個に増えた(許すのは {known} 個)。新しい画面の状態は \
             Workspace に足さず、文書ごと・窓全体・部品ごとの struct に置く",
            names.len()
        )],
        std::cmp::Ordering::Less => vec![format!(
            "画面の Workspace の欄が {} 個に減った。xtask/src/layers.rs の WORKSPACE_FIELDS を直す",
            names.len()
        )],
        std::cmp::Ordering::Equal => vec![],
    });
    out
}

/// 汎用の加工の crate が機械を名指ししている所。
pub(super) fn machine_names(root: &Path, g: &Graph, known: &[(&str, &str, &str)]) -> Vec<String> {
    let mut out = vec![];
    let mut used = BTreeSet::new();
    for &n in g.p.keys().filter(|n| is_generic_cam(n)) {
        for (rel, _, src) in product_sources(root, g.dir(n)) {
            for at in machine_words(&src) {
                if known.iter().any(|(k, f, _)| *k == n && *f == rel) {
                    used.insert((n.to_string(), rel.clone()));
                    continue;
                }
                out.push(format!(
                    "{n}/{rel}:{} が機械を名指ししている。機械の値は機械の crate(kmb-post-…)に置き、\
                     組み立てがマシンライブラリに入れる",
                    src.line_of(at)
                ));
            }
        }
    }
    for (k, f, _) in known {
        if !used.contains(&(k.to_string(), f.to_string())) {
            out.push(format!(
                "{k}/{f} はもう機械を名指ししていない。xtask/src/layers.rs の MACHINE_KNOWN から消す"
            ));
        }
    }
    out
}

/// 文字列のリテラルで、拡張子 `json` を名指ししている所(`"json"` と、`.json` で終わる名前)の位置。
pub(super) fn json_names(src: &Source) -> Vec<usize> {
    src.literals
        .iter()
        .filter(|l| l.text == "json" || l.text.ends_with(".json"))
        .map(|l| l.at)
        .collect()
}

/// 文書の拡張子を字で書いている所(8-5)。文書の保存・開く・リンク・一覧は `kmb_core::DOCUMENT_EXTENSION`
/// と `DOCUMENT_EXTENSIONS` を使う。文書でない JSON(設定・控え・索引・パラメータの表など)は `known` に
/// 数で載せる。数が増えたら落とし(文書の拡張子なら定数を使う。文書でなければ表を直す)、減ったら表を直させる。
pub(super) fn document_extensions(
    root: &Path,
    g: &Graph,
    known: &[(&str, &str, usize)],
) -> Vec<String> {
    let mut out = vec![];
    let mut seen = BTreeSet::new();
    // 点検の道具(xtask)は文書を読み書きしない
    for &n in g.p.keys().filter(|&&n| n != "xtask") {
        for (rel, _, src) in product_sources(root, g.dir(n)) {
            let at = json_names(&src);
            let allowed = known
                .iter()
                .find(|(k, f, _)| *k == n && *f == rel)
                .map_or(0, |e| e.2);
            if !at.is_empty() || allowed > 0 {
                seen.insert((n.to_string(), rel.clone()));
            }
            if at.len() > allowed {
                let lines: Vec<String> = at.iter().map(|&a| src.line_of(a).to_string()).collect();
                out.push(format!(
                    "{n}/{rel} が拡張子 json を字で書いている({} 個、許すのは {allowed} 個。行 {})。\
                     文書のファイルなら kmb_core::DOCUMENT_EXTENSION(開く側は DOCUMENT_EXTENSIONS)を使う。\
                     文書でない JSON なら xtask/src/layers.rs の JSON_NAME_KNOWN を直す",
                    at.len(),
                    lines.join("・")
                ));
            } else if at.len() < allowed {
                out.push(format!(
                    "{n}/{rel} の拡張子 json の字が {} 個に減った。xtask/src/layers.rs の JSON_NAME_KNOWN を直す",
                    at.len()
                ));
            }
        }
    }
    for (k, f, _) in known {
        if !seen.contains(&(k.to_string(), f.to_string())) {
            out.push(format!(
                "{k}/{f} はもう無い。xtask/src/layers.rs の JSON_NAME_KNOWN から消す"
            ));
        }
    }
    out
}

/// 機能を足す変更(`Edit::Insert`)と、ほかの機能の中身を直す変更(`Edit::SetData`)を同じ関数で組んでいる
/// 所の(関数の名前, 位置)(8-3)。関数の境目は次の `fn` までと見る(字面の目安)。
pub(super) fn insert_and_rewrites(code: &str) -> Vec<(String, usize)> {
    let starts: Vec<usize> = code
        .match_indices("fn ")
        .map(|(i, _)| i)
        .filter(|&i| {
            !code[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
        .collect();
    let mut out = vec![];
    for (k, &a) in starts.iter().enumerate() {
        let b = starts.get(k + 1).copied().unwrap_or(code.len());
        let body = &code[a..b];
        if body.contains("Edit::Insert") && body.contains("Edit::SetData") {
            let name: String = body[3..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            out.push((name, a));
        }
    }
    out
}

/// 後から足す機能が、前の機能の中身を書き換えていないか(8-3。消せば元に戻るようにする)。機能を足す
/// 変更とほかの機能を直す変更を同じ関数で組む所は `known`(crate, 関数, 理由)に無ければ落とす。
pub(super) fn rewrites_on_insert(
    root: &Path,
    g: &Graph,
    known: &[(&str, &str, &str)],
) -> Vec<String> {
    let mut out = vec![];
    let mut seen = BTreeSet::new();
    for &n in g.p.keys() {
        for (rel, _, src) in product_sources(root, g.dir(n)) {
            for (name, at) in insert_and_rewrites(&src.code) {
                if known.iter().any(|(k, f, _)| *k == n && *f == name) {
                    seen.insert((n.to_string(), name));
                    continue;
                }
                out.push(format!(
                    "{n}/{rel}:{} の {name} が、機能を足す変更とほかの機能の中身を直す変更を一緒に組んでいる。\
                     後から足す機能は前の機能を書き換えない(消せば元に戻る。足す機能の側に持つ)。\
                     足す機能が前を書き換えるのでなければ xtask/src/layers.rs の REWRITE_KNOWN に理由を書く",
                    src.line_of(at)
                ));
            }
        }
    }
    for (k, f, _) in known {
        if !seen.contains(&(k.to_string(), f.to_string())) {
            out.push(format!(
                "{k} の {f} はもう足す変更と直す変更を一緒に組んでいない。xtask/src/layers.rs の REWRITE_KNOWN から消す"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_rewrite_in_one_function_is_found() {
        let code = "fn a() { Edit::Insert {}; }\nfn b() { Edit::Insert {}; Edit::SetData {}; }\n\
                    fn c() { Edit::SetData {}; }\nfn d_fn() { Edit::SetData {}; Edit::Insert {}; }";
        let names: Vec<String> = insert_and_rewrites(code).into_iter().map(|x| x.0).collect();
        assert_eq!(names, ["b", "d_fn"]);
    }

    #[test]
    fn json_extension_names_are_found_in_literals_only() {
        let src = Source::new(
            "// 文書は .json\nconst A: &str = \"json\";\nlet p = dir.join(\"index.json\");\n\
             let q = format!(\"{stem}.json\");\nlet r = \"json の中身\";\nlet s = kmb_core::DOCUMENT_EXTENSION;\n",
        );
        let lines: Vec<usize> = json_names(&src).iter().map(|&a| src.line_of(a)).collect();
        assert_eq!(lines, [2, 3, 4]);
    }

    #[test]
    fn kernel_uses_are_found_as_whole_words() {
        let code = "let k = ctx.kernel();\nlet h: ShapeHandle = x;\nlet a = ShapeHandles;\nfn f(k: &dyn Kernel) {}\nctx.kernel_name()";
        let at = kernel_uses(code);
        assert_eq!(at.len(), 3, "{at:?}");
    }

    #[test]
    fn raw_argument_reads_are_found_across_lines() {
        let code = "let a = cx.arg(\"k\");\nlet b = cx.args.get(\"k\");\nlet c = cx\n    .args\n    .get(\"k\");";
        // 改行をまたいで書いた物も数える
        assert_eq!(raw_reads(&Source::new(code).code).len(), 2);
        assert_eq!(raw_reads("cx.args .get(x)").len(), 1);
        assert!(raw_reads("cx.args.clone()").is_empty());
        assert!(raw_reads("g.args.get(k)").is_empty());
        assert!(raw_reads("ctx.args.get(k)").is_empty());
        assert!(raw_reads("cx.args_x.get(k)").is_empty());
    }

    #[test]
    fn direct_file_writes_are_found_across_lines() {
        let code = "std::fs::write(p, b)?;\nfs::write\n    (p, b);\nstd::fs::File::create(p);\nkmb_session::write_output(p, b);";
        assert_eq!(direct_writes(&Source::new(code).code).len(), 3);
        assert!(direct_writes("my_fs::write(p)").is_empty());
        assert!(direct_writes("fs::write_all(p)").is_empty());
    }

    #[test]
    fn machine_names_are_found_in_code_and_ids_but_not_in_comments_or_sentences() {
        let src = Source::new(
            "// Carvera Air の値\nfn carvera_air() {}\nconst A: &str = \"carvera-air\";\n\
             const B: &str = \"機械の命令(Carvera Air は G32)\";\n",
        );
        let lines: Vec<usize> = machine_words(&src)
            .iter()
            .map(|&a| src.line_of(a))
            .collect();
        assert_eq!(lines, [2, 3]);
        assert!(is_generic_cam("kmb-strategy-face") && is_generic_cam("kmb-post"));
        assert!(!is_generic_cam("kmb-post-carvera-air") && !is_generic_cam("kmb-bundle"));
    }

    #[test]
    fn mutable_statics_are_found_but_constants_and_lifetimes_are_not() {
        let code = "static SCENE: RwLock<Option<Scene>> = RwLock::new(None);\n\
                    static TABLE: [f64; 3] = [1.0, 2.0, 3.0];\n\
                    fn f(s: &'static str) {}\n\
                    thread_local! { static READ: RefCell<Vec<u8>> = RefCell::new(vec![]); }\n\
                    static mut COUNT: u32 = 0;\n\
                    pub static N: AtomicU32 = AtomicU32::new(0);";
        let names: Vec<String> = mutable_statics(code).into_iter().map(|(_, n)| n).collect();
        assert_eq!(names, ["SCENE", "READ", "COUNT", "N"]);
    }

    #[test]
    fn edit_builds_count_the_change_builders() {
        let code = "edit::rename(d, a); kmb_session::edit::set_components(l, v); edit::GroupMode::Exact;\n\
                    my_edit::x(); Transaction::new(l, ops); edit::ParamInfo::default()";
        assert_eq!(edit_builds(code).len(), 3);
    }

    #[test]
    fn op_uses_count_only_the_transaction_op() {
        let code = "Op::AddParam { .. }; ToolOp::Cut; kmb_core::Op::SetBodyName; MyOp::X";
        assert_eq!(op_uses(code).len(), 2);
    }

    #[test]
    fn recompute_calls_are_found_as_whole_names() {
        let code = "doc.take_cache(); recompute_watched(&mut d, k, &w);\nlet x = my_take_cache(); hold_shapes_of(a); d.hold_shapes(v)";
        assert_eq!(calls_of(code, "take_cache").len(), 1);
        assert_eq!(calls_of(code, "recompute_watched").len(), 1);
        assert_eq!(calls_of(code, "hold_shapes").len(), 1);
    }

    #[test]
    fn struct_fields_reads_the_top_level_names() {
        let code = "pub struct Workspace {\n    #[allow(dead_code)]\n    pub doc: Document,\n    \
                    factory: Option<Arc<dyn Fn() -> Arc<dyn Kernel>>>,\n    \
                    rows: Vec<(FeatureId, [f32; 2])>,\n    last: u8\n}\n\
                    struct Other { x: u8 }";
        assert_eq!(
            struct_fields(code, "pub struct Workspace").unwrap(),
            ["doc", "factory", "rows", "last"]
        );
    }

    #[test]
    fn only_the_numeric_crates_may_use_nalgebra() {
        let toml = "[package]\nname = \"x\"\n\n[dependencies]\nkmb-core.workspace = true\n\
                    nalgebra.workspace = true\n\n[dev-dependencies]\nproptest = \"1\"\n";
        assert!(normal_dependency(toml, "nalgebra"));
        assert!(!normal_dependency(toml, "proptest"));
        assert!(!normal_dependency(toml, "nalgebra-sparse"));
        let dev = "[dependencies]\nkmb-core = { path = \"../kmb-core\" }\n[dev-dependencies]\nnalgebra = \"0.33\"\n";
        assert!(!normal_dependency(dev, "nalgebra"));
        assert!(normal_dependency(
            "[dependencies]\nnalgebra = { version = \"0.33\" }\n",
            "nalgebra"
        ));
    }
}
