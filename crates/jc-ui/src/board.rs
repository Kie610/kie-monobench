//! 画板「JC の画面」で Kie と決めたツールバー(`board/toolbar.json`)と実物を比べる。
//!
//! 比べるのは、まとまりの順、前に出す大きなボタンと 2 段の小さなボタン(順・絵の土台と変わる
//! 部分・役)、▾ の一覧にある道具。まだ入っていない物は `board/known-diffs.txt` の既知の差に
//! 書く。差が直ったら表から消し、表は画板のデータを直す時のほかは増やさない(`cargo xtask verify`
//! の点検「画板の並び」が分かれ目の版と比べて落とす)。決めた並びがコードに入らないまま検査が
//! 全部通ることを防ぐ。
//!
//! 画板の「新」の印の道具(画板を作った時の JC に無く、作る候補)は、JC に無いうちは比べない。
//! ▾ の、何かを選んだ時だけ使える道具(画板の薄い物)も比べない。
//! 画板を作った後に Kie が決めた違いは、データの方を直し、その項目の `decided` に日付と決めた事を書く。
//! 試験は jc-ui(設計・加工)と jc-ui-sketch(スケッチ)にある。

use crate::icons::{self, IconRole};
use serde::Deserialize;
use std::collections::BTreeSet;

const BOARD: &str = include_str!("../board/toolbar.json");
const KNOWN: &str = include_str!("../board/known-diffs.txt");

#[derive(Deserialize)]
struct Board {
    spaces: Vec<Space>,
}

#[derive(Deserialize)]
struct Space {
    space: String,
    groups: Vec<BoardGroup>,
}

#[derive(Deserialize)]
struct BoardGroup {
    name: String,
    front: Vec<Tool>,
    #[serde(default)]
    small: Vec<Tool>,
    menu: Vec<Row>,
}

#[derive(Deserialize)]
struct Tool {
    label: String,
    role: String,
    b: String,
    a: String,
    #[serde(default)]
    new: bool,
}

#[derive(Deserialize)]
struct Row {
    label: String,
    #[serde(default)]
    new: bool,
    /// 何かを選んだ時だけ使える道具(画板の薄い物)。選ぶ物で出る出ないが変わるので比べない
    #[serde(default)]
    needs_selection: bool,
}

/// 実物の 1 つのまとまり。前と小さなボタンは(名前, アイコンの名前)、▾ は名前。
#[derive(Debug, Default)]
pub struct Actual {
    pub name: String,
    pub front: Vec<(String, String)>,
    pub small: Vec<(String, String)>,
    pub menu: Vec<String>,
}

/// 作業空間 `space`(画板の名前: 設計・加工・スケッチ)の、画板と実物の差。
pub fn diffs(space: &str, actual: &[Actual]) -> Vec<String> {
    let board: Board = serde_json::from_str(BOARD).expect("board/toolbar.json が読めない");
    let Some(b) = board.spaces.into_iter().find(|s| s.space == space) else {
        return vec![format!("{space}: 画板のデータに無い")];
    };
    let all: BTreeSet<&str> = actual
        .iter()
        .flat_map(|g| {
            g.front
                .iter()
                .chain(&g.small)
                .map(|(l, _)| l.as_str())
                .chain(g.menu.iter().map(String::as_str))
        })
        .collect();
    // 「新」の印の道具は、JC にあれば比べる
    let wanted = |label: &str, new: bool| !new || all.contains(label);
    let mut out = vec![];
    let names: Vec<&str> = actual.iter().map(|g| g.name.as_str()).collect();
    let board_names: Vec<&str> = b.groups.iter().map(|g| g.name.as_str()).collect();
    for n in &board_names {
        if !names.contains(n) {
            out.push(format!("{space}: まとまり「{n}」が無い"));
        }
    }
    for n in &names {
        if !board_names.contains(n) {
            out.push(format!("{space}: 画板に無いまとまり「{n}」がある"));
        }
    }
    if !same_order(&board_names, &names) {
        out.push(format!("{space}: まとまりの順が画板と違う"));
    }
    for bg in &b.groups {
        let Some(g) = actual.iter().find(|g| g.name == bg.name) else {
            continue;
        };
        let at = format!("{space}/{}", bg.name);
        for (kind, want, have) in [
            ("前", &bg.front, &g.front),
            ("小さなボタン", &bg.small, &g.small),
        ] {
            let want: Vec<&Tool> = want.iter().filter(|t| wanted(&t.label, t.new)).collect();
            let want_names: Vec<&str> = want.iter().map(|t| t.label.as_str()).collect();
            let have_names: Vec<&str> = have.iter().map(|(l, _)| l.as_str()).collect();
            for t in &want_names {
                if !have_names.contains(t) {
                    out.push(format!("{at}: 「{t}」が{kind}に無い"));
                }
            }
            for t in &have_names {
                if !want_names.contains(t) {
                    out.push(format!("{at}: 画板に無い「{t}」が{kind}にある"));
                }
            }
            if !same_order(&want_names, &have_names) {
                out.push(format!("{at}: {kind}の順が画板と違う"));
            }
            for t in &want {
                if let Some((_, icon)) = have.iter().find(|(l, _)| *l == t.label) {
                    out.extend(icon_diffs(&at, t, icon));
                }
            }
        }
        let menu: BTreeSet<&str> = g.menu.iter().map(String::as_str).collect();
        for r in bg
            .menu
            .iter()
            .filter(|r| !r.needs_selection && wanted(&r.label, r.new))
        {
            if !menu.contains(r.label.as_str()) {
                out.push(format!("{at}: ▾ に「{}」が無い", r.label));
            }
        }
    }
    out
}

/// 既知の差(`board/known-diffs.txt`)のうち `space` の物。1 行に 1 つ、「差 # 理由」。
pub fn known(space: &str) -> Vec<String> {
    KNOWN
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split(" # ").next().unwrap_or(l).trim().to_string())
        .filter(|l| l.starts_with(&format!("{space}/")) || l.starts_with(&format!("{space}:")))
        .collect()
}

/// 差が既知の差の表とちょうど合うか。合わなければ、何を直すかを書いた文。
pub fn check(space: &str, actual: &[Actual]) -> Result<(), String> {
    let now: BTreeSet<String> = diffs(space, actual).into_iter().collect();
    let known: BTreeSet<String> = known(space).into_iter().collect();
    let fresh: Vec<&String> = now.difference(&known).collect();
    let gone: Vec<&String> = known.difference(&now).collect();
    if fresh.is_empty() && gone.is_empty() {
        return Ok(());
    }
    let mut msg = format!("{space} のツールバーが画板「JC の画面」と合わない。\n");
    if !fresh.is_empty() {
        msg += "画板と違う所(実物を画板に合わせる。画板を直すなら crates/jc-ui/board/toolbar.json も):\n";
        for d in fresh {
            msg += &format!("  {d}\n");
        }
    }
    if !gone.is_empty() {
        msg += "直った差(crates/jc-ui/board/known-diffs.txt から消す):\n";
        for d in gone {
            msg += &format!("  {d}\n");
        }
    }
    Err(msg)
}

/// 共通の物だけを残した順が同じか。
fn same_order(want: &[&str], have: &[&str]) -> bool {
    let a: Vec<&&str> = want.iter().filter(|x| have.contains(x)).collect();
    let b: Vec<&&str> = have.iter().filter(|x| want.contains(x)).collect();
    a == b
}

fn icon_diffs(at: &str, t: &Tool, icon: &str) -> Vec<String> {
    let label = &t.label;
    let Some((base, accent)) = icons::source(icon) else {
        return vec![format!("{at}: 「{label}」の絵が無い")];
    };
    let mut out = vec![];
    if squash(base) != squash(&t.b) {
        out.push(format!("{at}: 「{label}」の絵の土台が画板と違う"));
    }
    let role = match t.role.as_str() {
        "make" => Some(IconRole::Make),
        "modify" => Some(IconRole::Modify),
        "repeat" => Some(IconRole::Repeat),
        "define" => Some(IconRole::Define),
        _ => None,
    };
    if accent.map(|(r, _)| r) != role {
        out.push(format!("{at}: 「{label}」の役が画板と違う"));
    }
    if squash(accent.map_or("", |(_, d)| d)) != squash(&t.a) {
        out.push(format!("{at}: 「{label}」の絵の変わる部分が画板と違う"));
    }
    out
}

/// ツールバーの道具 `tools`(名前, アイコンの名前)のうち、絵が役の色の 2 層の決まりに合わない物。
/// 道具の絵は、変わる部分(役)を持つか、役を持たない 1 層の絵の表(調べる・出すなど)にあるかの
/// どちらか 1 つ(Kie 2026-10-21: 役の色を、作る・変える・増やす・決めるの道具の変わる部分に差す)。
/// 画板に絵の無い ▾ の一覧の道具も見る。
pub fn layer_gaps(tools: &[(String, String)]) -> Vec<String> {
    let mut out = BTreeSet::new();
    // 絵の名前の無い項目(▾ の一覧だけの、字だけの行)は描く絵が無いので見ない
    for (label, icon) in tools.iter().filter(|(_, i)| !i.is_empty()) {
        let accent = icons::source(icon).and_then(|(_, a)| a).is_some();
        let one = icons::ONE_LAYER.contains(&icon.as_str());
        match (accent, one) {
            (false, false) => {
                out.insert(format!("「{label}」({icon}): 役の色の変わる部分が無い"));
            }
            (true, true) => {
                out.insert(format!(
                    "「{label}」({icon}): 変わる部分があるのに 1 層の表にもある"
                ));
            }
            _ => {}
        }
    }
    out.into_iter().collect()
}

/// 道筋の空白をそろえる。
fn squash(d: &str) -> String {
    d.split_whitespace().collect::<Vec<_>>().join(" ")
}
