//! 入出力の一覧(docs/入出力.md、提案 N2)。操作と引数・単位、役割の印、設定、文書の形式と版、
//! CLI の終了の番号、MCP の道具を、組み立て(jc-bundle)の登録とこのサーバの道具から作る。
//! AGENTS.md の入出力の節は決まりだけを書き、一覧はここを指す(手で書いた一覧は崩れるため)。
//!
//! 書き直すのは `cargo xtask io`(中身は `JC_BLESS=1 cargo test -p jc-mcp --lib io_reference`)。
//! 古いままだと jc-mcp の試験が落ちる。

use jc_core::ProblemKind;
use jc_session::Headless;
use std::collections::BTreeMap;

/// docs/入出力.md の中身。
pub fn io_reference() -> String {
    let h = Headless::new(jc_bundle::modules());
    let mut out = String::from(
        "# 入出力の一覧(生成)\n\n\
         `cargo xtask io` が組み立ての登録と MCP の道具から作る。手で直さない(古いと jc-mcp の試験が\n\
         落ちる)。決まりは AGENTS.md「4. 入出力の約束」。\n",
    );

    // 操作と引数(画面・CLI の `kmb actions`・MCP の `actions`・コマンド欄が同じ宣言を使う)
    let actions: Vec<jc_core::Action> = h
        .actions(None)
        .into_iter()
        .map(|mut a| {
            // 使えるかは文脈で変わるので、一覧には出さない
            a.disabled = None;
            a
        })
        .collect();
    out += &format!(
        "\n## 操作と引数({} 個)\n\n長さは mm、角度は deg で読む。単位つきの式も受ける。宣言に無い鍵は誤り。\n\n```text\n",
        actions.len()
    );
    out += &jc_session::run_args::actions_text(&actions);
    out += "```\n";

    // 役割の印(画面・CLI・MCP が名前でなく役割で探す物)
    let settings = h.modules().extensions.settings();
    let mut roles: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut note = |role: Option<String>, who: String| {
        if let Some(r) = role {
            roles.entry(r).or_default().push(who);
        }
    };
    for a in &actions {
        note(
            a.role.map(|r| format!("action/{}", r.0)),
            format!("操作 {}", a.id),
        );
    }
    for d in h.modules().registry.descriptors() {
        note(
            d.role.map(|r| format!("kind/{}", r.0)),
            format!("種類 {}", d.type_id),
        );
    }
    for s in &settings {
        note(
            s.role.map(|r| format!("settings/{}", r.0)),
            format!("設定 {}", s.id),
        );
    }
    out += &format!(
        "\n## 役割の印({} 個)\n\n| 役割 | 名乗る物 |\n|---|---|\n",
        roles.len()
    );
    for (r, who) in &roles {
        out += &format!("| `{r}` | {} |\n", who.join("・"));
    }

    // 設定(画面の設定・CLI・MCP の set_settings が同じ宣言を使う)
    out += &format!(
        "\n## 設定({} 個)\n\n| ID | 見出し | 欄 |\n|---|---|---|\n",
        settings.len()
    );
    for s in &settings {
        let fields: Vec<String> = s
            .fields
            .iter()
            .map(|f| format!("`{}` {}", f.key, f.label))
            .collect();
        out += &format!("| `{}` | {} | {} |\n", s.id, s.label, fields.join("・"));
    }

    out += &format!(
        "\n## 文書の形式と版\n\n- 文書は JSON。版は {}(`jc_core::FORMAT_VERSION`)。古い版は 1 版ずつ移して読み、\
         新しい版は読まない。\n",
        jc_core::FORMAT_VERSION
    );
    out += "- タイムラインに乗らないモジュールの中身は区画(鍵・版はモジュールが持つ)。知らない区画と項目は書き戻す。\n\n\
             | 区画の鍵 | 中身 |\n|---|---|\n";
    for (key, label) in h.modules().registry.sections() {
        out += &format!("| `{key}` | {label} |\n");
    }

    out +=
        "\n## 誤りの種類と CLI の終了の番号\n\n| 種類(MCP の `kind`) | 終了の番号 |\n|---|---|\n";
    for k in kinds() {
        let name = serde_json::to_value(k).unwrap_or_default();
        out += &format!(
            "| `{}` | {} |\n",
            name.as_str().unwrap_or_default(),
            k.exit_code()
        );
    }

    let tools = crate::Server::tools();
    out += &format!(
        "\n## MCP の道具({} 個)\n\n| 道具 | 説明 |\n|---|---|\n",
        tools.len()
    );
    for (name, about) in &tools {
        let first = about.lines().next().unwrap_or_default().replace('|', "\\|");
        out += &format!("| `{name}` | {first} |\n");
    }
    out
}

/// 誤りの種類を全部(種類を足すと、ここの match が足りずに組み立てが止まる)。
fn kinds() -> Vec<ProblemKind> {
    let all = [
        ProblemKind::Input,
        ProblemKind::Compute,
        ProblemKind::Found,
        ProblemKind::Unavailable,
        ProblemKind::File,
    ];
    for k in all {
        match k {
            ProblemKind::Input
            | ProblemKind::Compute
            | ProblemKind::Found
            | ProblemKind::Unavailable
            | ProblemKind::File => {}
        }
    }
    all.to_vec()
}
