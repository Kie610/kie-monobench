//! 層の点検: 依存の向きと、部品の依存に出ない結び付き(他の層の名前を文字列で書く・他の層の
//! crate を `pub use` で出し直す)を、組み立てずに調べる。`cargo xtask verify` が毎回、どの crate を
//! 選んでも(選ばなくても)最初に流す。
//!
//! 各 crate は Cargo.toml の `[package.metadata.jc]` の `layer` で層を名乗る。依存は `cargo metadata`
//! の正規の表で、通常・ビルド用・開発用(テスト用)の全部を見る。依存先の機能をそろえるだけの
//! `jc-workspace-hack`(層は support、中身がない)は数えない。

use crate::source::{Source, rust_files};
use crate::{Kind, Package};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

mod code;
mod names;

use code::{
    direct_output_writes, document_extensions, machine_names, numeric_libraries, process_statics,
    raw_arg_reads, rewrites_on_insert, screen_edits, screen_ops, screen_recomputes,
    session_only_calls, strategy_kernel_uses, workspace_fields,
};
use names::{foreign_names, modules_named_once, names_table};

/// 層の名前。
pub const LAYERS: [&str; 14] = [
    "core",
    "interchange",
    "inspect",
    "kernel",
    "cad",
    "cam",
    "analysis",
    "io",
    "ui",
    "session",
    "test",
    "assembly",
    "app",
    "support",
];

/// 各層が通常の依存(とビルド用の依存)に持ってよい層。
/// 画面の拡張(jc-ui-*)が扱う種類のモジュール(cad・cam)を使うのは、下の個別の点検で絞る。
pub fn allowed(layer: &str) -> &'static [&'static str] {
    match layer {
        "core" => &["core"],
        // 受け渡しの型(CAD が出し、CAM が読む)。コアの型だけで書く
        "interchange" => &["core"],
        // 形を調べる計算(測る・絵にする)。網(コアの型)だけを受け取り、画面と画面を持たない作業が共通で使う
        "inspect" => &["core"],
        "kernel" => &["core", "kernel"],
        "cad" => &["core", "interchange", "cad"],
        "cam" => &["core", "interchange", "cam", "io"],
        // 解析(強度・熱など)。形はコアの網とカーネルの trait だけで読み、CAD・CAM を知らない
        "analysis" => &["core", "analysis"],
        // 書き出し。絵と動画は描き手(inspect の jc-render)で描いてから書く。役割の印の語彙は
        // 受け渡しの型(jc-interchange の role)で名乗る
        "io" => &["core", "interchange", "inspect"],
        // 受け渡しの型(jc-interchange)の役割の印と描き方の役割の語彙で、モジュールを探し、色を決める
        // (印の語彙をコアに置かない)
        "session" => &["core", "interchange", "inspect"],
        "test" => &["core"],
        "ui" => &[
            "core",
            "interchange",
            "inspect",
            "session",
            "ui",
            "cad",
            "cam",
        ],
        "assembly" => &[
            "core",
            "interchange",
            "kernel",
            "cad",
            "cam",
            "analysis",
            "io",
            "session",
            "test",
            "assembly",
        ],
        // CLI・MCP の短い命令は、操作を役割の印(jc-interchange の role)で探す
        "app" => &["core", "interchange", "session", "ui", "assembly"],
        _ => &[],
    }
}

/// 開発用の依存(テスト)に、通常の表に足して持ってよい層: 試験の部品(test)と、見本の文書と
/// 全部のモジュールを並べた組み立て(assembly)。カーネルを直接知るのは下の `DEV_KERNEL` だけ。
pub const DEV_EXTRA: [&str; 2] = ["test", "assembly"];

/// 開発用の依存でカーネルを直接使ってよい crate(カーネルどうしの比べ・形の計算の試験)。
pub const DEV_KERNEL: [&str; 1] = ["jc-solid"];

/// 開発用の依存で、上の表に入らない今の辺。(crate, 依存先, 理由)。直したら行を消す
/// (消し忘れると「もう無い」で落ちる)。
const DEV_KNOWN: &[(&str, &str, &str)] = &[
    // 画面の写真の試験(tests/screens.rs)が、書き出した 3MF の中身を形式の crate で読んで確かめる
    ("jc-app", "jc-io-stl", "写真の試験が書き出した 3MF を読む"),
];

/// 画面の外枠の下に置く部品(画面の拡張ではない。外枠と同じく、機能のモジュールを知らない)。
const SCREEN_PARTS: [&str; 1] = ["jc-ui-view3d"];

/// 画面の拡張(jc-ui-*)が通常の依存に持ってよい機能のモジュール(扱う種類のモジュールだけ)。
const UI_EXTENSIONS: &[(&str, &[&str])] = &[
    ("jc-ui-sketch", &["jc-sketch", "jc-sketch-3d"]),
    ("jc-ui-tools", &["jc-cam-tool"]),
    ("jc-ui-text", &["jc-text"]),
];

/// 他の層の crate を出し直してよい所(層が違っても、使う側がその層を直接使ってよい物)。
/// (crate, 出し直す crate, 理由)
const REEXPORT_KNOWN: &[(&str, &str, &str)] = &[];

/// 戦略がカーネルに直に触っている今の所。(crate, ソースの相対パス, 理由)。直したら行を消す。
const STRATEGY_KERNEL_KNOWN: &[(&str, &str, &str)] = &[];

/// 汎用の加工の crate が機械を名指ししている今の所。(crate, ソースの相対パス, 理由)。直したら行を消す。
const MACHINE_KNOWN: &[(&str, &str, &str)] = &[
    (
        "jc-cam-setup",
        "src/lib.rs",
        "機械の欄が無い古い文書の機械の ID(文書の互換の値。値はライブラリから読む)",
    ),
    (
        "jc-cam-tool",
        "src/lib.rs",
        "機械の工具一式の出し口。次に機械を足す時に機械の crate へ移す(試験の移動に承認が要る)",
    ),
    (
        "jc-cam-tool",
        "src/carvera.rs",
        "機械の工具一式。次に機械を足す時に機械の crate へ移す(試験の移動に承認が要る)",
    ),
];

/// 他の層の名前を書いている今の所。(crate, ソースの相対パス, 名前)。直したら行を消す。
const NAME_KNOWN: &[(&str, &str, &str)] = &[];

/// 調べた結果: 守れていない所と、もう無いのに残った例外の行。
pub fn check(root: &Path, packages: &[Package]) -> Vec<String> {
    let g = Graph::new(packages);
    let mut out = g.dependency_problems();
    out.extend(reexports(root, &g));
    let names = names_table(root);
    out.extend(foreign_names(root, &g, &names));
    out.extend(modules_named_once(root, &g));
    out.extend(raw_arg_reads(root, &g));
    out.extend(direct_output_writes(root, &g));
    out.extend(strategy_kernel_uses(root, &g, STRATEGY_KERNEL_KNOWN));
    out.extend(machine_names(root, &g, MACHINE_KNOWN));
    out.extend(numeric_libraries(root, &g));
    out.extend(process_statics(root, &g, STATIC_KNOWN));
    out.extend(screen_ops(root, &g, SCREEN_OP_KNOWN));
    out.extend(screen_edits(root, &g, SCREEN_EDIT_KNOWN));
    out.extend(session_only_calls(root, &g, SESSION_ONLY_KNOWN));
    out.extend(screen_recomputes(root, &g));
    out.extend(document_extensions(root, &g, JSON_NAME_KNOWN));
    out.extend(rewrites_on_insert(root, &g, REWRITE_KNOWN));
    out.extend(workspace_fields(root, WORKSPACE_FIELDS));
    let all: Vec<(String, String, crate::source::Source)> =
        g.p.keys()
            .flat_map(|&n| {
                product_sources(root, g.dir(n))
                    .into_iter()
                    .map(move |(rel, _, src)| (n.to_string(), rel, src))
            })
            .collect();
    out.extend(crate::copies::check(root, &all));
    out
}

struct Graph<'a> {
    p: BTreeMap<&'a str, &'a Package>,
    /// テスト用の依存の例外(本物は `DEV_KNOWN`。点検の試験は空で作る)
    dev_known: &'a [(&'a str, &'a str, &'a str)],
}

impl<'a> Graph<'a> {
    fn new(packages: &'a [Package]) -> Self {
        Graph {
            p: packages.iter().map(|p| (p.name.as_str(), p)).collect(),
            dev_known: DEV_KNOWN,
        }
    }

    fn layer(&self, n: &str) -> &str {
        self.p.get(n).and_then(|p| p.layer.as_deref()).unwrap_or("")
    }

    /// 依存(jc-workspace-hack を除く)。`dev` が偽なら通常とビルド用だけ。
    fn deps(&self, n: &str, dev: bool) -> Vec<&'a str> {
        let Some(p) = self.p.get(n) else {
            return vec![];
        };
        let mut v: Vec<&str> = p
            .edges
            .iter()
            .filter(|e| e.name != "jc-workspace-hack")
            .filter(|e| dev == (e.kind == Kind::Dev))
            .map(|e| e.name.as_str())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    fn dependency_problems(&self) -> Vec<String> {
        let mut out = vec![];
        for (&n, p) in &self.p {
            let l = p.layer.as_deref().unwrap_or("");
            if !LAYERS.contains(&l) {
                out.push(format!(
                    "{n} の Cargo.toml に [package.metadata.jc] layer が無いか、知らない層 {l:?}(使える層: {LAYERS:?})"
                ));
            }
        }
        // コアは式の crate だけに依存し、式の crate は何にも依存しない
        for d in self.deps("jc-core", false) {
            if d != "jc-expr" {
                out.push(format!("コアが {d} に依存している"));
            }
        }
        for d in self.deps("jc-expr", false) {
            out.push(format!("式の crate が {d} に依存している"));
        }

        let mut dev_used = BTreeSet::new();
        for &n in self.p.keys() {
            let l = self.layer(n);
            for d in self.deps(n, false) {
                let dl = self.layer(d);
                if !allowed(l).contains(&dl) {
                    out.push(format!(
                        "{n}({l})が {d}({dl})に依存している。{l} が使ってよい層は {:?}",
                        allowed(l)
                    ));
                }
                // カーネルを使うのは組み立てだけ
                if dl == "kernel" && n != "jc-bundle" && l != "kernel" {
                    out.push(format!("{n} がカーネル {d} を直接使っている"));
                }
                // 組み立てに依存するのは実行ファイルだけ
                if d == "jc-bundle" && l != "app" {
                    out.push(format!("{n} が組み立てに依存している"));
                }
            }
            for d in self.deps(n, true) {
                let dl = self.layer(d);
                let ok = if dl == "kernel" {
                    l == "kernel" || l == "assembly" || DEV_KERNEL.contains(&n)
                } else {
                    allowed(l).contains(&dl) || DEV_EXTRA.contains(&dl)
                };
                if ok {
                    continue;
                }
                if self.dev_known.iter().any(|(a, b, _)| *a == n && *b == d) {
                    dev_used.insert((n, d));
                    continue;
                }
                out.push(format!(
                    "{n}({l})のテスト用の依存 {d}({dl})が層の表に合わない。テストが使ってよいのは \
                     {:?} と {DEV_EXTRA:?}(カーネルは {DEV_KERNEL:?} とカーネル・組み立てだけ)。\
                     要るなら xtask/src/layers.rs の DEV_KNOWN に理由を書く",
                    allowed(l)
                ));
            }
            // 加工の操作と共通部品は戦略を知らない(戦略の一覧は組み立てが名乗りで渡す。テストだけは使ってよい)
            if l == "cam" && !n.starts_with("jc-strategy-") {
                for d in self.deps(n, false) {
                    if d.starts_with("jc-strategy-") {
                        out.push(format!(
                            "{n} が戦略 {d} に依存している。戦略の違いは名乗り(jc_cam_setup::Strategy)の欄で受け取る"
                        ));
                    }
                }
            }
            // 加工の戦略どうしは依存しない(どの依存でも)
            if n.starts_with("jc-strategy-") {
                for d in self.deps(n, false).into_iter().chain(self.deps(n, true)) {
                    if d.starts_with("jc-strategy-") {
                        out.push(format!("{n} が {d} に依存している(戦略どうし)"));
                    }
                }
                // 機械の値はホストが StrategyInput.setup.machine に入れる(戦略は機械の表とポストを知らない)
                for d in self.deps(n, false) {
                    if d == "jc-post" || d.starts_with("jc-post-") {
                        out.push(format!(
                            "{n} が {d} に依存している。戦略が使う機械の値は jc_cam_setup::MachineView に足して入力で受け取る"
                        ));
                    }
                }
            }
        }
        for (a, b, _) in self.dev_known {
            if !dev_used.contains(&(*a, *b)) {
                out.push(format!(
                    "{a} のテスト用の依存 {b} はもう表に合っているか無い。xtask/src/layers.rs の DEV_KNOWN から消す"
                ));
            }
        }

        // 画面の外枠は、機能の一覧・設定欄・操作をコア経由で受け取るだけ(CAD・CAM・カーネルを知らない)。
        // 3D のビュー(jc-ui-view3d)と測る計算(jc-measure)と描き手(jc-render)は外枠の部品。
        // 受け渡しの語彙(jc-interchange。描き方の役割など)と外観の語彙(jc-look。材料と外観の表、
        // 光源とデカールの型)は読んでよい
        for d in self.deps("jc-ui", false) {
            if !(matches!(
                d,
                "jc-core"
                    | "jc-interchange"
                    | "jc-look"
                    | "jc-session"
                    | "jc-measure"
                    | "jc-render"
            ) || SCREEN_PARTS.contains(&d))
            {
                out.push(format!("画面が {d} に依存している"));
            }
        }
        // 3D のビューは画面の外枠にも機能にも依存しない。測る計算と描き手は egui も知らない
        for d in self.deps("jc-ui-view3d", false) {
            if !matches!(
                d,
                "jc-core" | "jc-interchange" | "jc-look" | "jc-measure" | "jc-render"
            ) {
                out.push(format!("3D のビューが {d} に依存している"));
            }
        }
        // 描き手(jc-render)は形を絵にした PNG を書くので、コアの層の形式の小部品(jc-format)も使ってよい。
        // どちらも材料と外観の表(コアの層の jc-look)は読んでよい
        for (n, ok) in [
            ("jc-measure", &["jc-core", "jc-look"][..]),
            ("jc-render", &["jc-core", "jc-format", "jc-look"][..]),
        ] {
            for d in self.deps(n, false) {
                if !ok.contains(&d) {
                    out.push(format!("{n} が {d} に依存している(コアだけを知る)"));
                }
            }
        }
        // 専用の編集器(jc-ui-*)は、画面の外枠とその部品と、扱う種類のモジュールだけに依存する
        for &n in self.p.keys().filter(|n| is_ui_extension(n)) {
            let own: &[&str] = UI_EXTENSIONS
                .iter()
                .find(|(e, _)| *e == n)
                .map_or(&[], |(_, m)| m);
            for d in self.deps(n, false) {
                if d.starts_with("jc-ui-") && !SCREEN_PARTS.contains(&d) {
                    out.push(format!("{n} が別の編集器 {d} に依存している"));
                }
                if matches!(self.layer(d), "cad" | "cam" | "io" | "analysis") && !own.contains(&d) {
                    out.push(format!(
                        "{n} が扱う種類のモジュールでない {d} に依存している(扱うのは {own:?}。\
                         xtask/src/layers.rs の UI_EXTENSIONS)"
                    ));
                }
            }
        }
        // 実行ファイルは、コア・ファイルの扱い・組み立て(と画面)だけを使う。機能のモジュールを直接
        // 呼ぶと、CLI・MCP・画面で同じ処理が別々に育つため。役割の印の語彙(jc-interchange)は読んでよい
        for app in self.p.keys().filter(|n| self.layer(n) == "app") {
            for d in self.deps(app, false) {
                if !(matches!(d, "jc-core" | "jc-interchange" | "jc-session" | "jc-bundle")
                    || (*app == "jc-app" && d.starts_with("jc-ui")))
                {
                    out.push(format!("{app} が {d} に直接依存している"));
                }
            }
        }
        // ソリッドの機能(jc-cad-*)は、共通部品(jc-cad)とコアと受け渡しの型と材料の表(jc-look)だけを使う
        for &n in self.p.keys().filter(|n| n.starts_with("jc-cad")) {
            for d in self.deps(n, false) {
                if !matches!(d, "jc-core" | "jc-cad" | "jc-interchange" | "jc-look") {
                    out.push(format!("{n} が {d} に依存している(ソリッドの機能どうし)"));
                }
            }
        }
        out
    }

    fn dir(&self, n: &str) -> &str {
        self.p.get(n).map_or("", |p| p.dir.as_str())
    }
}

fn is_ui_extension(n: &str) -> bool {
    n.starts_with("jc-ui-") && !SCREEN_PARTS.contains(&n)
}

/// 試験を除いた製品のソース(crate の中の相対パスと、読んだ物)。名前に `test` を含むファイルは
/// 読まない。`#[cfg(test)]` の項目は、範囲だけを空白にする。
fn product_sources(root: &Path, dir: &str) -> Vec<(String, String, Source)> {
    let base = root.join(dir);
    let mut out = vec![];
    for f in rust_files(&base.join("src")) {
        let name = f.file_name().unwrap_or_default().to_string_lossy();
        if name.contains("test") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&f) else {
            continue;
        };
        let rel = f
            .strip_prefix(&base)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        let mut src = Source::new(&text);
        let ranges = src.test_ranges();
        src.literals
            .retain(|l| !ranges.iter().any(|&(a, b)| a <= l.at && l.at < b));
        let mut code = src.code.into_bytes();
        for (a, b) in ranges {
            for c in &mut code[a..b] {
                if *c != b'\n' {
                    *c = b' ';
                }
            }
        }
        src.code = String::from_utf8_lossy(&code).into_owned();
        out.push((rel, text, src));
    }
    out
}

/// 行列の計算の crate(nalgebra)を通常の依存に持ってよい crate。数値の解き方はここに集め、作り
/// 直しを 1 か所で比べる(policy.md)。
const NUMERIC_OWNERS: &[&str] = &["jc-sketch-solver", "jc-fea", "jc-flow"];

/// プロセス全体の書き換えられる状態を置かない層(モジュールの層)。設定や控えは拡張の中に持つ
/// (static だと、同じプロセスで作業場を作り直しても前の値が残る)。
const NO_STATIC_LAYERS: [&str; 7] = [
    "interchange",
    "inspect",
    "kernel",
    "cad",
    "cam",
    "analysis",
    "io",
];

/// プロセス全体の書き換えられる状態を置いている今の所。(crate, ソースの相対パス, static の名前,
/// 理由)。直したら行を消す。
const STATIC_KNOWN: &[(&str, &str, &str, &str)] = &[
    (
        "jc-fea",
        "src/tet10.rs",
        "RULE",
        "一度だけ作る不変の表(書き換えない)",
    ),
    (
        "jc-fea",
        "src/tet10.rs",
        "M",
        "一度だけ作る不変の表(書き換えない)",
    ),
    (
        "jc-fea",
        "src/tet10.rs",
        "B",
        "一度だけ作る不変の表(書き換えない)",
    ),
    (
        "jc-render",
        "src/envmap.rs",
        "MAP",
        "一度だけ作る不変の環境の絵(書き換えない)",
    ),
    ("jc-io-render", "src/mf.rs", "N", "一時ファイルの名前の採番"),
    (
        "jc-kernel-smlib",
        "src/ffi.rs",
        "SMLIB",
        "SMLib は同時に呼べないので呼び出しを並べる錠",
    ),
];

/// 画面とアプリの層で取り消しの変更(`Op::`)をその場で組んでよい所。(crate, ソースの相対パス,
/// 数)。画面の変更は jc-session の edit・params・Headless を通す(CLI・MCP と同じ道。CLI には同じ
/// 試験がある)。ここへ足さない。
const SCREEN_OP_KNOWN: &[(&str, &str, usize)] = &[];

/// 画面とアプリの層で文書の変更を jc-session の edit(と `Transaction::new`)でその場で組んでいる今の
/// 所。(crate, ソースの相対パス, 数)。ここで組んだ変更は CLI・MCP から呼べない。減らすだけで足さない
/// (新しい操作は `Headless` の口に置き、画面もそれを呼ぶ)。
const SCREEN_EDIT_KNOWN: &[(&str, &str, usize)] = &[
    ("jc-ui", "src/animation/animation_ui.rs", 1),
    ("jc-ui", "src/assembly/components_ui.rs", 2),
    ("jc-ui", "src/assembly/joint_drag_ui.rs", 3),
    ("jc-ui", "src/design/browser_ui.rs", 1),
    ("jc-ui", "src/design/config_ui.rs", 2),
    ("jc-ui", "src/design/doc_ui.rs", 6),
    ("jc-ui", "src/design/feature_clip_ui.rs", 1),
    ("jc-ui", "src/design/panel_ui.rs", 1),
    ("jc-ui", "src/design/param_views_ui.rs", 1),
    ("jc-ui", "src/design/params_ui.rs", 2),
    ("jc-ui", "src/design/timeline_ui.rs", 15),
    ("jc-ui", "src/design/view_ui.rs", 2),
    ("jc-ui", "src/frame_ui/sketch_band.rs", 1),
    ("jc-ui", "src/live_ui.rs", 2),
    ("jc-ui", "src/machining/reuse_ui.rs", 2),
    ("jc-ui", "src/menus_ui.rs", 17),
    ("jc-ui", "src/workspace/edits.rs", 7),
    ("jc-ui", "src/workspace/selection.rs", 1),
];

/// `Headless` の文書を変える口のうち、画面が呼ばない今の物(CLI・MCP からしかできない、または画面は
/// 別の道で同じ変更を組んでいる)。減らすだけで足さない。例外は文書を変えない `hold_recompute`
/// (まとめて実行の間だけ作り直しを後へ回す。画面は裏で作り直すので要らない)。
const SESSION_ONLY_KNOWN: &[&str] = &[
    "add_feature",
    "add_param",
    "add_param_info",
    "add_param_view",
    "alloc_group_id",
    "duplicate_component",
    "edit",
    "group",
    "hold_recompute",
    "import_params",
    "move_bodies",
    "move_component",
    "move_features",
    "param_view_rows",
    "paste_features",
    "remove_named_view",
    "remove_param",
    "remove_param_view",
    "rename_named_view",
    "rename_param",
    "rename_param_view",
    "restore_param",
    "restore_version",
    "run",
    "save",
    "save_named_view",
    "set_body_material",
    "set_param",
    "set_param_info",
    "set_properties",
    "set_value",
    "squash_since",
    "stand_up",
    "ungroup",
];

/// 機能を足す変更とほかの機能を直す変更を同じ関数で組んでいる今の所(8-3)。(crate, 関数, 理由)。
/// ここへ足さない(後から足す機能が前の機能を書き換えない)。
const REWRITE_KNOWN: &[(&str, &str, &str)] = &[
    (
        "jc-core",
        "apply_edits",
        "モジュールの返した変更を当てる所(どの変更も通る)",
    ),
    (
        "jc-asm",
        "replace",
        "リンクを最新にする・元を選び直す: 直すのはリンクの機能そのもので、足すのは元に増えたボディ",
    ),
    (
        "jc-asm",
        "drag_position",
        "引いて動かす: 最後の位置の機能を直すか、新しく足すかのどちらか 1 つ",
    ),
    (
        "jc-strategy-drill",
        "propose",
        "穴の加工の提案: 前の加工の工具を直す操作と、足りない加工を足す操作を 1 回で返す(足す機能は前を書き換えない)",
    ),
];

/// 文書でない JSON のファイルの名前を字で書いている所(設定・控え・索引・パラメータの表など)。
/// (crate, ソースの相対パス, 数)。文書の拡張子は `jc_core::DOCUMENT_EXTENSION` を使い、ここへ足さない。
const JSON_NAME_KNOWN: &[(&str, &str, usize)] = &[
    ("jc-asm", "src/link.rs", 1),           // 部品を移した記録 moved.json
    ("jc-cam-commands", "src/reuse.rs", 5), // 加工のテンプレートのファイル
    ("jc-cam-tool", "src/exchange.rs", 2),  // 工具ライブラリのファイル
    ("jc-core", "src/document.rs", 1),      // 文書の拡張子の定数そのもの
    ("jc-post-carvera-air", "src/machine.rs", 1), // 機械の台の穴の表
    ("jc-session", "src/crash.rs", 5),      // 落ちた時の記録
    ("jc-session", "src/filters.rs", 1),    // 絞り込みの設定
    ("jc-session", "src/gallery.rs", 2),    // 絵の索引
    ("jc-session", "src/history.rs", 2),    // 版の履歴の索引
    ("jc-session", "src/lib.rs", 5),        // 最近の一覧・自動保存の写し・設定
    ("jc-session", "src/live.rs", 1),       // AI の操作の控え
    ("jc-session", "src/organize.rs", 1),   // 部品を移した記録 moved.json
    ("jc-session", "src/param_io.rs", 1),   // パラメータの表
    ("jc-session", "src/projects.rs", 1),   // プロジェクトの一覧
    ("jc-session", "src/startup.rs", 4),    // 起動の印と設定のファイル
    ("jc-ui", "src/board.rs", 1),           // 画板の並び
    ("jc-ui", "src/design/feature_ui.rs", 1), // egui の記憶の鍵
    ("jc-ui", "src/design/param_views_ui.rs", 2), // パラメータの表
    ("jc-ui", "src/layout.rs", 1),          // 画面の配置
    ("jc-ui", "src/themes.rs", 3),          // 色のテーマ
    ("jc-ui-tools", "src/lib.rs", 4),       // 工具ライブラリ
];

/// 画面の `Workspace` の欄の今の数。新しい画面の状態は Workspace に足さず、文書ごと・窓全体・
/// 部品ごとの struct に置く。欄を減らしたら数を直す。
const WORKSPACE_FIELDS: usize = 11;
const WORKSPACE_FILE: &str = "crates/jc-ui/src/lib.rs";
/// 文書ごとの画面の状態(タブを替えると丸ごと入れ替える)。egui の型を持たない(画面の位置や
/// 描き方の控えは画面の側に置く)。
const DOC_STATE_FILE: &str = "crates/jc-ui/src/doc_state.rs";

/// 他の層の crate を `pub use` で出し直している所。依存の図が実際の結び付きを隠し、使う側が層の
/// 表で許されない層を使えてしまう。コアの出し直しは誰でもコアを使えるので数えない。
fn reexports(root: &Path, g: &Graph) -> Vec<String> {
    let mut out = vec![];
    let mut used = BTreeSet::new();
    for &n in g.p.keys() {
        let l = g.layer(n);
        for (rel, _, src) in product_sources(root, g.dir(n)) {
            for (at, _) in src.code.match_indices("pub use ") {
                let rest = src.code[at + "pub use ".len()..].trim_start();
                let rest = rest.strip_prefix("::").unwrap_or(rest);
                let ident: String = rest
                    .bytes()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_')
                    .map(char::from)
                    .collect();
                let target = ident.replace('_', "-");
                if !ident.starts_with("jc_") || !g.p.contains_key(target.as_str()) {
                    continue;
                }
                let tl = g.layer(&target);
                if tl == l || tl == "core" {
                    continue;
                }
                if REEXPORT_KNOWN
                    .iter()
                    .any(|(a, b, _)| *a == n && *b == target)
                {
                    used.insert((n.to_string(), target));
                    continue;
                }
                out.push(format!(
                    "{n}({l})/{rel}:{} が別の層の {target}({tl})を pub use で出し直している。\
                     使う側が {target} に直接依存する(層の表で許されないなら、{n} の API に包む)",
                    src.line_of(at)
                ));
            }
        }
    }
    for (a, b, _) in REEXPORT_KNOWN {
        if !used.contains(&(a.to_string(), b.to_string())) {
            out.push(format!(
                "{a} はもう {b} を出し直していない。xtask/src/layers.rs の REEXPORT_KNOWN から消す"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Edge;

    fn pkg(name: &str, layer: &str, deps: &[(&str, Kind)]) -> Package {
        Package {
            name: name.into(),
            dir: format!("crates/{name}"),
            deps: deps.iter().map(|(d, _)| d.to_string()).collect(),
            layer: Some(layer.into()),
            description: None,
            edges: deps
                .iter()
                .map(|(d, k)| Edge {
                    name: d.to_string(),
                    kind: *k,
                })
                .collect(),
        }
    }

    use Kind::{Build, Dev, Normal};

    fn problems(ps: &[Package]) -> Vec<String> {
        Graph {
            dev_known: &[],
            ..Graph::new(ps)
        }
        .dependency_problems()
    }

    #[test]
    fn normal_and_build_dependencies_follow_the_layer_table() {
        let ok = [
            pkg("jc-expr", "core", &[]),
            pkg("jc-core", "core", &[("jc-expr", Normal)]),
            pkg("jc-cad-x", "cad", &[("jc-core", Normal)]),
        ];
        assert_eq!(problems(&ok), Vec::<String>::new());
        let mut bad = ok.to_vec();
        bad.push(pkg("jc-cam-y", "cam", &[("jc-cad-x", Build)]));
        let p = problems(&bad);
        assert!(
            p.iter()
                .any(|s| s.contains("jc-cam-y(cam)が jc-cad-x(cad)")),
            "{p:?}"
        );
        // 層の名乗りが無い
        let mut none = ok.to_vec();
        none[2].layer = None;
        assert!(problems(&none)[0].contains("layer が無い"));
    }

    #[test]
    fn cam_operations_do_not_depend_on_strategies_except_in_tests() {
        let ok = [
            pkg("jc-expr", "core", &[]),
            pkg("jc-core", "core", &[("jc-expr", Normal)]),
            pkg("jc-strategy-x", "cam", &[("jc-core", Normal)]),
            pkg(
                "jc-cam-ops",
                "cam",
                &[("jc-core", Normal), ("jc-strategy-x", Dev)],
            ),
        ];
        assert_eq!(problems(&ok), Vec::<String>::new());
        let mut bad = ok.to_vec();
        bad[3] = pkg("jc-cam-ops", "cam", &[("jc-strategy-x", Normal)]);
        let p = problems(&bad);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("jc-cam-ops が戦略 jc-strategy-x に依存している"));
    }

    #[test]
    fn strategies_do_not_depend_on_posts_except_in_tests() {
        let ok = [
            pkg("jc-expr", "core", &[]),
            pkg("jc-core", "core", &[("jc-expr", Normal)]),
            pkg("jc-post", "cam", &[("jc-core", Normal)]),
            pkg(
                "jc-strategy-x",
                "cam",
                &[("jc-core", Normal), ("jc-post", Dev)],
            ),
        ];
        assert_eq!(problems(&ok), Vec::<String>::new());
        let mut bad = ok.to_vec();
        bad[3] = pkg("jc-strategy-x", "cam", &[("jc-post", Normal)]);
        let p = problems(&bad);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("jc-strategy-x が jc-post に依存している"));
    }

    #[test]
    fn test_dependencies_may_use_the_assembly_but_not_the_kernel() {
        let base = [
            pkg("jc-expr", "core", &[]),
            pkg("jc-core", "core", &[("jc-expr", Normal)]),
            pkg("jc-kernel-k", "kernel", &[("jc-core", Normal)]),
            pkg("jc-bundle", "assembly", &[("jc-kernel-k", Normal)]),
            pkg("jc-contract", "test", &[("jc-core", Normal)]),
        ];
        let mut ws = base.to_vec();
        ws.push(pkg(
            "jc-ui",
            "ui",
            &[
                ("jc-core", Normal),
                ("jc-bundle", Dev),
                ("jc-contract", Dev),
            ],
        ));
        assert_eq!(problems(&ws), Vec::<String>::new());
        ws.push(pkg("jc-cad-x", "cad", &[("jc-kernel-k", Dev)]));
        let p = problems(&ws);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("テスト用の依存 jc-kernel-k"));
        // 通常の依存で組み立てを使うのは実行ファイルだけ
        ws.pop();
        ws.push(pkg("jc-ui-x", "ui", &[("jc-bundle", Normal)]));
        assert!(problems(&ws).iter().any(|s| s.contains("組み立てに依存")));
    }

    #[test]
    fn screen_extensions_use_only_the_module_they_edit() {
        let ws = [
            pkg("jc-expr", "core", &[]),
            pkg("jc-core", "core", &[("jc-expr", Normal)]),
            pkg("jc-sketch", "cad", &[("jc-core", Normal)]),
            pkg("jc-cad-extrude", "cad", &[("jc-core", Normal)]),
            pkg("jc-ui-view3d", "ui", &[("jc-core", Normal)]),
            pkg(
                "jc-ui-sketch",
                "ui",
                &[
                    ("jc-sketch", Normal),
                    ("jc-ui-view3d", Normal),
                    ("jc-cad-extrude", Normal),
                ],
            ),
        ];
        let p = problems(&ws);
        // 3D のビュー(外枠の部品)は使ってよい。扱わないモジュールは落とす
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("jc-ui-sketch が扱う種類のモジュールでない jc-cad-extrude"));
    }
}
