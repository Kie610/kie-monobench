//! 別ファイルの部品をリンクで組み込む。
//!
//! - リンクの機能 `asm.link@1`([`Link`]): 元の文書の中身の写し(加工を除く)を持ち、計算の中で
//!   入れ子に計算して、元の文書の見えているボディを `part:<元の最初の機能>` の出力に出す。写しを
//!   持つので、元のファイルを動かしたり消したりしても開ける(STEP の読み込みと同じ考え)。計算の中で
//!   ファイルは読まない(再計算は文書だけで決まる)。
//! - ボディの機能 `asm.link_body@1`([`LinkBody`]): リンクの出力 1 つをボディとして出す。挿入すると
//!   ボディごとに 1 つ足し、まとめて元のファイルの名前の部品に入れる。ボディなので、ジョイント・干渉・
//!   部品表・加工のセットアップでそのまま選べる。
//! - 元が変わったかは、画面が時々 [`kmb_core::Extension::links`] で聞く([`state`]: 元のファイルを
//!   読んで写しと比べる)。「最新にする」
//!   (`asm.link_update`)は写しを差し替える取り消せる変更、「この版に固定」(`asm.link_pin`)は
//!   変わっても知らせない、「リンクを切る」(`asm.link_break`)は元を見ない写しにする。
//!   「元のファイルを選び直す…」(`asm.link_relink`)は、移した・差し替えた元を選ばせて写しを差し替える。
//!   最新にする時も選び直す時も、ボディは元の最初の機能の番号で対応させ、番号で見つからない物は
//!   同じ名前のボディに付け替える(別のファイルに替えても、同じ名前の部品が付いて来る)。

use kmb_cad::solid;
use kmb_core::{
    Action, Context, Document, Edit, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef,
    LinkInfo, LinkStatus, Outcome, Outputs, Place, Registry, Value, body_labels, body_root,
    category, visible_bodies,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const LINK: &str = "asm.link@1";
pub const LINK_BODY: &str = "asm.link_body@1";
/// 別ファイルの部品をリンクで挿入する操作(ファイルを選ばせる)。
pub const LINK_INSERT: &str = "asm.link_insert";
/// 写しを元のファイルの今の中身に差し替える操作(選んでいるリンクと、元のファイル)。
pub const LINK_UPDATE: &str = "asm.link_update";
/// この版に固定する・固定を外す操作。
pub const LINK_PIN: &str = "asm.link_pin";
/// リンクを切る操作(元を見ない写しにする)。
pub const LINK_BREAK: &str = "asm.link_break";
/// 元のファイルを選び直す操作(ファイルを選ばせる。元が見つからない時にも使える)。
pub const LINK_RELINK: &str = "asm.link_relink";
/// 新しい元に見つからなかったボディの番号(元の文書の機能の番号は 1 から。0 の出力は無いので誤りになり、
/// 新しい元で同じ番号になった別のボディを黙って出さない)。
pub const GONE: u64 = 0;
/// リンクの出力の名前の頭(後ろは元の文書のボディの最初の機能の番号)。
pub const PART: &str = "part:";

/// リンクの機能のデータ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// 元のファイル(選んだ時の絶対パス)
    pub path: String,
    /// 元のファイルの名前。`path` に無い時は、文書と同じフォルダのこの名前を探す
    #[serde(default)]
    pub file: String,
    /// 元の文書の中身の写し(保存形式の JSON。加工の機能は除く)
    pub doc: serde_json::Value,
    /// この版に固定(元が変わっても知らせない)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// リンクを切った(元を見ない写し)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub broken: bool,
}

/// リンクのボディの機能のデータ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkBody {
    /// リンクの機能
    pub link: FeatureId,
    /// 元の文書のボディ(最初に作った機能の番号)
    pub part: u64,
}

/// 元の文書の写しを計算する。見えているボディを(元の最初の機能、名前、形)で。
fn evaluate_copy(
    ctx: &EvalCtx<'_>,
    doc: &serde_json::Value,
) -> Result<(Outputs, Vec<String>), EvalError> {
    let d = Document::from_json(&doc.to_string())
        .map_err(|e| EvalError::InvalidData(format!("組み込んだ文書の写しを読めません: {e}")))?;
    let data = d.data();
    let report = ctx.recompute_nested(data)?;
    let reg = ctx.registry();
    let bodies = visible_bodies(data, reg, &report);
    let ids: Vec<FeatureId> = bodies.iter().map(|b| b.0).collect();
    let names = body_labels(data, reg, &ids);
    let mut out = Outputs::new();
    for ((id, h), name) in bodies.iter().zip(&names) {
        let root = body_root(data, reg, *id);
        log::debug!("組み込んだ文書のボディ「{name}」(元の機能 {})", root.0);
        out.insert(format!("{PART}{}", root.0), Value::Shape(*h));
    }
    let mut warn = Vec::new();
    if report.error_count() > 0 {
        warn.push(format!(
            "元の文書の写しに計算できない所が {} 個あります(元の文書を開いて直してください)",
            report.error_count()
        ));
    }
    if bodies.is_empty() {
        warn.push("元の文書にボディがありません".into());
    }
    Ok((out, warn))
}

impl Feature for Link {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let (mut out, warn) = evaluate_copy(ctx, &self.doc)?;
        kmb_core::add_warnings(&mut out, warn);
        Ok(out)
    }
}

impl Feature for LinkBody {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![InputRef::Output {
            feature: self.link,
            slot: format!("{PART}{}", self.part),
        }])
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let v = ctx
            .output(self.link, &format!("{PART}{}", self.part))
            .map_err(|_| {
                EvalError::Other(
                    "元の文書にこのボディがありません(元で消したか、作り直した)".into(),
                )
            })?
            .clone();
        Ok([("body".to_string(), v)].into())
    }
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(LINK, "リンクした部品")
            .icon("link")
            .hint("別のファイルの部品の写し。元が変わるとブラウザに印が出て、部品の右クリックで最新にできる")
            .field(Field::info("file", "元のファイル"))
            .field(Field::bool("pinned", "この版に固定").help(
                "元のファイルが変わっても知らせず、最新にもしない(固定を外すと、また知らせる)",
            ))
            .no_raw_editor(),
        |d| Ok(Box::new(kmb_cad::parse::<Link>(d)?)),
    );
    reg.register_described(
        solid(LINK_BODY, "リンクした部品のボディ")
            .icon("link")
            .hint("リンクした部品の中のボディ 1 つ。形は元のファイルで直す")
            .parent("link")
            .no_raw_editor(),
        |d| Ok(Box::new(kmb_cad::parse::<LinkBody>(d)?)),
    );
}

/// 文書の JSON から、組み込む写しを作る(加工の機能を除く)。
pub fn design_copy(src: &str, registry: &Registry) -> Result<serde_json::Value, String> {
    let d = Document::from_json(src).map_err(|e| format!("文書として読めません: {e}"))?;
    let mut data = d.data().clone();
    let before = data.features.len();
    data.features.retain(|f| {
        registry.descriptor(&f.type_id).is_none_or(|d| {
            ![category::TOOLS, category::SETUP, category::OPERATION].contains(&d.category.as_str())
        })
    });
    if data.features.len() != before {
        log::info!(
            "組み込む写しから加工の機能 {} 個を除いた",
            before - data.features.len()
        );
    }
    // 除いた後の位置に合わせる(ロールバックの棒は外す: 元の最後の形を組み込む)
    data.rollback = None;
    Ok(serde_json::to_value(&data).expect("文書のデータは JSON にできる"))
}

/// リンクを選んでいる機能(リンクか、そのボディ)から、リンクの機能とデータ。
pub fn link_of(cx: &Context<'_>, id: FeatureId) -> Option<(FeatureId, Link)> {
    let e = cx.data.feature(id)?;
    let id = match e.type_id.as_str() {
        LINK => id,
        LINK_BODY => {
            serde_json::from_value::<LinkBody>(e.data.clone())
                .ok()?
                .link
        }
        _ => return None,
    };
    let e = cx.data.feature(id).filter(|e| e.type_id == LINK)?;
    Some((id, serde_json::from_value(e.data.clone()).ok()?))
}

/// リンクのボディの機能(リンク `link` の物。タイムラインの順)と、その元のボディの番号。
pub fn bodies_of(data: &kmb_core::DocumentData, link: FeatureId) -> Vec<(FeatureId, u64)> {
    data.features
        .iter()
        .filter(|f| f.type_id == LINK_BODY)
        .filter_map(|f| {
            let b: LinkBody = serde_json::from_value(f.data.clone()).ok()?;
            (b.link == link).then_some((f.id, b.part))
        })
        .collect()
}

/// 写し `doc` の見えているボディ(元の最初の機能の番号と名前、タイムラインの順)。写しを今のカーネルで
/// 計算する(作った形は、文書の次の再計算が捨てる)。
fn copy_bodies(doc: &serde_json::Value, cx: &Context<'_>) -> Result<Vec<(u64, String)>, String> {
    let d = Document::from_json(&doc.to_string()).map_err(|e| e.to_string())?;
    let data = d.data();
    let report = kmb_core::recompute_nested(data, cx.registry, cx.kernel.as_ref())
        .map_err(|e| e.to_string())?;
    let ids: Vec<FeatureId> = visible_bodies(data, cx.registry, &report)
        .iter()
        .map(|b| b.0)
        .collect();
    let names = body_labels(data, cx.registry, &ids);
    let mut out: Vec<(u64, String)> = ids
        .iter()
        .zip(names)
        .map(|(id, n)| (body_root(data, cx.registry, *id).0, n))
        .collect();
    out.sort_by_key(|(r, _)| data.index_of(FeatureId(*r)).unwrap_or(usize::MAX));
    Ok(out)
}

/// 写し `doc` の中(入れ子も)に、`path` を元にするリンクがあるか(組み込むと輪になる)。
fn links_to(doc: &serde_json::Value, path: &Path, depth: usize) -> bool {
    let Some(features) = doc.get("features").and_then(|f| f.as_array()) else {
        return false;
    };
    depth < kmb_core::NESTED_DEPTH
        && features
            .iter()
            .filter(|f| f.get("type_id").and_then(|t| t.as_str()) == Some(LINK))
            .filter_map(|f| serde_json::from_value::<Link>(f.get("data")?.clone()).ok())
            .any(|l| same_file(Path::new(&l.path), path) || links_to(&l.doc, path, depth + 1))
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// 元のファイルを読んで、組み込む写しと絶対パスを作る。組み込む文書の中(入れ子も)に、元自身か
/// 今の文書(`cx.doc_path`)を組み込んだリンクがあれば、輪になるので断る。
fn read_source(path: &Path, cx: &Context<'_>) -> Result<(serde_json::Value, PathBuf), String> {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if cx.doc_path.is_some_and(|d| same_file(d, &abs)) {
        return Err("開いている文書そのものは組み込めません(輪になります)".into());
    }
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("{} を読めません: {e}", path.display()))?;
    let doc = design_copy(&src, cx.registry).map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(back) = std::iter::once(abs.as_path())
        .chain(cx.doc_path)
        .find(|p| links_to(&doc, p, 0))
    {
        return Err(format!(
            "{} は {} をリンクで組み込んでいます(組み込むと輪になります)",
            path.display(),
            back.display()
        ));
    }
    Ok((doc, abs))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "リンクした部品".into())
}

/// リンクで挿入する: リンクの機能と、ボディごとのボディの機能を足し、ファイルの名前の部品にまとめる。
fn insert(cx: &Context<'_>) -> Result<Outcome, String> {
    let path = cx.file.ok_or("ファイルを選んでいません")?;
    let (doc, abs) = read_source(path, cx)?;
    let bodies = copy_bodies(&doc, cx)?;
    if bodies.is_empty() {
        return Err(format!("{} にはボディがありません", path.display()));
    }
    let name = stem(path);
    let link = Link {
        path: abs.to_string_lossy().to_string(),
        file: file_name(path),
        doc,
        pinned: false,
        broken: false,
    };
    log::info!(
        "{} をリンクで挿入する(ボディ {} 個)",
        abs.display(),
        bodies.len()
    );
    let mut edits = vec![Edit::Insert {
        type_id: LINK.into(),
        name: format!("{name}(リンク)"),
        data: serde_json::to_value(&link).map_err(|e| e.to_string())?,
    }];
    for (part, body) in &bodies {
        edits.push(Edit::Insert {
            type_id: LINK_BODY.into(),
            name: format!("{name}: {body}"),
            data: json!({"link": {"$new": 0}, "part": part}),
        });
    }
    edits.push(Edit::AddBodies {
        component: None,
        name: name.clone(),
        parent: None,
        bodies: bodies
            .iter()
            .enumerate()
            .map(|(i, (_, b))| (i + 1, b.clone()))
            .collect(),
    });
    Ok(Outcome::Change {
        label: format!("「{name}」をリンクで挿入"),
        edits,
    })
}

/// 選んでいるリンク(リンクか、そのボディの機能)。
fn selected_link(cx: &Context<'_>) -> Result<(FeatureId, Link), String> {
    cx.selected
        .and_then(|id| link_of(cx, id))
        .ok_or_else(|| "リンクした部品を選んでいません".into())
}

/// 最新にする: 写しを元のファイル(`cx.file`、無ければ保存した絶対パス)の今の中身に差し替える。
fn update(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, link) = selected_link(cx)?;
    if link.broken {
        return Err("リンクを切った部品です(元のファイルとはつながっていません)".into());
    }
    if link.pinned {
        return Err("この版に固定しています(固定を外すと最新にできます)".into());
    }
    let path = cx
        .file
        .map_or_else(|| PathBuf::from(&link.path), Path::to_path_buf);
    let name = stem(&path);
    replace(cx, id, link, &path, format!("「{name}」を最新にする"))
}

/// 元のファイルを選び直す: 選んだファイル(`cx.file`)を新しい元にして写しを差し替える。
/// 固定はそのまま残す(選び直しは使う人がはっきり頼んだ差し替え)。
fn relink(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, link) = selected_link(cx)?;
    if link.broken {
        return Err("リンクを切った部品です(元のファイルとはつながっていません)".into());
    }
    let path = cx.file.ok_or("元にするファイルを選んでいません")?;
    let name = stem(path);
    replace(cx, id, link, path, format!("「{name}」の元を選び直す"))
}

/// 写しを `path` の今の中身に差し替える(`label` は変更の名前)。ボディは、番号(元の最初の機能)と
/// 名前が同じ物、次に名前が同じ物(別のファイルでは番号が意味を持たない)、最後に同じファイルなら
/// 番号が同じ物(元で名前を変えた)の順に対応させる。それでも無いボディの機能は残し、計算の誤りで知らせ、
/// 名前を変更の名前に並べる(後の機能やジョイントの参照を黙って消さない)。
/// 新しく増えたボディはボディの機能を足して同じ部品に入れる。
fn replace(
    cx: &Context<'_>,
    id: FeatureId,
    link: Link,
    path: &Path,
    mut label: String,
) -> Result<Outcome, String> {
    let (doc, abs) = read_source(path, cx)?;
    let bodies = copy_bodies(&doc, cx)?;
    if bodies.is_empty() {
        return Err(format!("{} にはボディがありません", path.display()));
    }
    let have = bodies_of(cx.data, id);
    // 前の写しのボディの名前(番号で見つからない物を名前で探すため)
    let old: HashMap<u64, String> = copy_bodies(&link.doc, cx)
        .unwrap_or_default()
        .into_iter()
        .collect();
    // 前の写しに無い番号(前に見つからなかった物)は、ボディの機能の名前(「部品: ボディ」)の後ろで
    let names: HashMap<FeatureId, String> = have
        .iter()
        .filter_map(|(b, _)| {
            let n = &cx.data.feature(*b)?.name;
            Some((*b, n.rsplit_once(": ").map_or(n.as_str(), |x| x.1).to_string()))
        })
        .collect();
    let have: Vec<(FeatureId, u64, String)> = have
        .iter()
        .map(|(b, q)| {
            let n = old.get(q).or(names.get(b)).cloned();
            (*b, *q, n.unwrap_or_else(|| format!("ボディ {q}")))
        })
        .collect();
    // 対応(ボディの機能 → 新しい元のボディの番号)。決まらない物は None
    let mut to: Vec<Option<u64>> = vec![None; have.len()];
    let mut taken: Vec<u64> = Vec::new();
    // 対応させる順: 0 番号と名前、1 名前、2 番号
    let fits = |pass: usize, q: u64, m: &str, (p, n): &(u64, String)| match pass {
        0 => *p == q && n == m,
        1 => n == m,
        _ => *p == q,
    };
    // 番号だけで対応させるのは同じファイルの時だけ(別のファイルの番号は偶然そろう)
    let same = same_file(Path::new(&link.path), &abs);
    for pass in 0..if same { 3 } else { 2 } {
        for (i, (_, q, m)) in have.iter().enumerate() {
            if to[i].is_some() {
                continue;
            }
            if let Some((p, _)) = bodies
                .iter()
                .find(|b| !taken.contains(&b.0) && fits(pass, *q, m, b))
            {
                taken.push(*p);
                to[i] = Some(*p);
            }
        }
    }
    let mut moved = Vec::new();
    let mut gone = Vec::new();
    for ((body, q, m), t) in have.iter().zip(&to) {
        match t {
            Some(p) if p == q => {}
            Some(p) => moved.push((*body, *p)),
            None => {
                gone.push(m.clone());
                if *q != GONE {
                    moved.push((*body, GONE));
                }
            }
        }
    }
    let name = stem(path);
    let new = Link {
        path: abs.to_string_lossy().to_string(),
        file: file_name(path),
        doc,
        ..link
    };
    let mut edits = vec![Edit::SetData {
        id,
        data: serde_json::to_value(&new).map_err(|e| e.to_string())?,
    }];
    for (body, part) in &moved {
        edits.push(Edit::SetData {
            id: *body,
            data: json!({"link": id, "part": part}),
        });
    }
    let added: Vec<&(u64, String)> = bodies.iter().filter(|(p, _)| !taken.contains(p)).collect();
    for (part, body) in &added {
        edits.push(Edit::Insert {
            type_id: LINK_BODY.into(),
            name: format!("{name}: {body}"),
            data: json!({"link": id, "part": part}),
        });
    }
    if !added.is_empty() {
        let component = have
            .first()
            .and_then(|(b, _, _)| cx.data.component_of(*b))
            .map(|c| c.id);
        edits.push(Edit::AddBodies {
            component,
            name: name.clone(),
            parent: None,
            bodies: added
                .iter()
                .enumerate()
                .map(|(i, (_, b))| (i, b.clone()))
                .collect(),
        });
    }
    log::info!(
        "{label}(リンク {}): {}(ボディ 増えた {}、付け替えた {}、見つからない {})",
        id.0,
        abs.display(),
        added.len(),
        moved.len(),
        gone.len()
    );
    if !gone.is_empty() {
        label.push_str(&format!("(元に見つからないボディ: {})", gone.join("、")));
    }
    Ok(Outcome::Change { label, edits })
}

fn set_link(cx: &Context<'_>, label: &str, f: impl FnOnce(&mut Link)) -> Result<Outcome, String> {
    let (id, mut link) = selected_link(cx)?;
    f(&mut link);
    Ok(Outcome::Change {
        label: label.into(),
        edits: vec![Edit::SetData {
            id,
            data: serde_json::to_value(&link).map_err(|e| e.to_string())?,
        }],
    })
}

/// リンクの操作。元のファイルを調べた結果を覚えておく。
#[derive(Default)]
pub struct LinkCommands {
    memo: Mutex<HashMap<FeatureId, (u64, LinkStatus, String)>>,
}

impl kmb_core::Extension for LinkCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let link = cx.selected.and_then(|id| link_of(cx, id));
        let none = link
            .is_none()
            .then_some("リンクした部品(か、そのボディ)を選んでいません");
        let broken = link
            .as_ref()
            .is_some_and(|(_, l)| l.broken)
            .then_some("リンクを切った部品です");
        let pinned = link.as_ref().is_some_and(|(_, l)| l.pinned);
        vec![
            Action::new(LINK_INSERT, "リンクで挿入…", "cad")
                .role(kmb_interchange::role::action::INSERT_LINK)
                .menu("挿入")
                .icon("link")
                .aliases("りんく リンク link そうにゅう 挿入 insert くみこみ 組み込み 部品 ぶひん 別のファイル")
                .hint("別のファイルの部品を、元とつながったまま挿入する。元を直すと印が出て、部品の右クリックで最新にできる")
                .input(kmb_core::Input::OpenFile {
                    title: "リンクで挿入".into(),
                    filter: "文書".into(),
                    extensions: kmb_core::DOCUMENT_EXTENSIONS
                        .iter()
                        .map(|e| e.to_string())
                        .collect(),
                }),
            Action::new(LINK_UPDATE, "最新にする", "cad")
                .menu("組立")
                .icon("link")
                .aliases("さいしん 最新 こうしん 更新 update りんく リンク")
                .hint("元のファイルの今の形に差し替える(取り消せる)")
                .at(&[Place::FeatureMenu])
                .disabled_if(none.or(broken).or(pinned.then_some(
                    "この版に固定しています(固定を外すと最新にできます)",
                ))),
            Action::new(LINK_RELINK, "元のファイルを選び直す…", "cad")
                .menu("組立")
                .icon("link")
                .aliases("えらびなおす 選び直す さしかえ 差し替え つけなおす 付け直す relink replace りんく リンク 元 もと")
                .hint("移した・差し替えた元のファイルを選んで、写しを差し替える。同じ名前のボディは付いて来て、見つからないボディは知らせる(取り消せる)")
                .at(&[Place::FeatureMenu])
                .disabled_if(none.or(broken))
                .input(kmb_core::Input::OpenFile {
                    title: "元のファイルを選び直す".into(),
                    filter: "文書".into(),
                    extensions: kmb_core::DOCUMENT_EXTENSIONS
                        .iter()
                        .map(|e| e.to_string())
                        .collect(),
                }),
            Action::new(
                LINK_PIN,
                if pinned { "固定解除" } else { "この版に固定" },
                "cad",
            )
            .menu("組立")
            .icon("pin")
            .aliases("こてい 固定 ぴん ピン pin ろっく ロック")
            .hint("固定すると、元のファイルが変わっても知らせず、最新にもしない")
            .at(&[Place::FeatureMenu])
            .disabled_if(none.or(broken)),
            Action::new(LINK_BREAK, "リンクを切る", "cad")
                .menu("組立")
                .icon("link")
                .aliases("りんくをきる リンクを切る break unlink")
                .hint("元のファイルとのつながりをやめ、今の写しのまま持つ(元が変わっても知らせない。取り消せる)")
                .at(&[Place::FeatureMenu])
                .disabled_if(none.or(broken)),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        Some(match id {
            LINK_INSERT => insert(cx),
            LINK_UPDATE => update(cx),
            LINK_RELINK => relink(cx),
            LINK_PIN => {
                let (_, l) = match selected_link(cx) {
                    Ok(x) => x,
                    Err(e) => return Some(Err(e)),
                };
                let label = if l.pinned {
                    "リンクの固定を外す"
                } else {
                    "リンクをこの版に固定"
                };
                set_link(cx, label, |l| l.pinned = !l.pinned)
            }
            LINK_BREAK => set_link(cx, "リンクを切る", |l| l.broken = true),
            _ => return None,
        })
    }

    fn links(&self, cx: &Context<'_>) -> Vec<LinkInfo> {
        link_infos(cx, &self.memo)
    }

    fn status(&self, cx: &Context<'_>) -> Vec<(String, serde_json::Value)> {
        let links = self.links(cx);
        if links.is_empty() {
            return Vec::new();
        }
        vec![(
            "links".into(),
            serde_json::to_value(links).expect("JSON にできる"),
        )]
    }

    fn summary(&self, cx: &Context<'_>, id: FeatureId) -> Option<String> {
        let (link_id, l) = link_of(cx, id)?;
        let n = bodies_of(cx.data, link_id).len();
        let how = if l.broken {
            "リンクを切った写し"
        } else if l.pinned {
            "この版に固定"
        } else {
            "リンク"
        };
        Some(format!("{}({how}、ボディ {n} 個)", l.path))
    }
}

/// 元のファイルの見つけ方: 保存した絶対パス、無ければそこから移した記録をたどった先、
/// 無ければ文書と同じフォルダの同じ名前。
pub fn resolve(link: &Link, doc_path: Option<&Path>) -> Option<PathBuf> {
    let abs = PathBuf::from(&link.path);
    if abs.is_file() {
        return Some(abs);
    }
    if let Some(p) = follow_moves(&abs) {
        return Some(p);
    }
    let name = if link.file.is_empty() {
        abs.file_name()?.to_os_string()
    } else {
        link.file.clone().into()
    };
    let near = doc_path?.parent()?.join(name);
    near.is_file().then_some(near)
}

/// モデル管理で移した・名前を変えたファイルの行き先を、元のフォルダの移した記録
/// (`<フォルダ>/.jc/moved.json`、kmb-session の `organize` が書く)からたどる。
/// 形は `{"moves":[{"from":"<元のファイル名>","to":"<移した先の絶対パス>","at":<秒>}]}`。
/// フォルダごと移した時は、その親の記録に `from` = フォルダの名前で残るので、上のフォルダの記録も見る。
/// 移した先からまた移していれば続けてたどる(輪にならないよう 16 回まで)。
pub fn follow_moves(path: &Path) -> Option<PathBuf> {
    #[derive(Deserialize)]
    struct Moved {
        from: String,
        to: PathBuf,
    }
    #[derive(Deserialize)]
    struct Moves {
        #[serde(default)]
        moves: Vec<Moved>,
    }
    let mut cur = path.to_path_buf();
    for _ in 0..16 {
        let next = cur.ancestors().find_map(|a| {
            let name = a.file_name()?.to_string_lossy().to_string();
            let file = a.parent()?.join(".jc").join("moved.json");
            let moves: Moves = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
            // 同じ名前は最後の物が正
            let to = moves.moves.into_iter().rev().find(|m| m.from == name)?.to;
            let rest = cur.strip_prefix(a).ok()?;
            Some(if rest.as_os_str().is_empty() {
                to
            } else {
                to.join(rest)
            })
        })?;
        if next.is_file() {
            return Some(next);
        }
        cur = next;
    }
    None
}

/// 元のファイルの中身 `src` と写しを比べる(加工の機能を除いた設計の部分だけ)。様子と短い説明。
pub fn state(link: &Link, src: Result<&str, String>, registry: &Registry) -> (LinkStatus, String) {
    if link.broken {
        return (LinkStatus::Broken, "リンクを切った写し".into());
    }
    let now = match src.and_then(|s| design_copy(s, registry)) {
        Ok(v) => v,
        Err(e) => {
            return (
                LinkStatus::Missing,
                format!("元のファイルを読めません(写しのまま使います): {e}"),
            );
        }
    };
    // 写しも同じ読み方を通して比べる(保存形式の版が上がった後でも同じ物は同じ)
    let copy = Document::from_json(&link.doc.to_string())
        .ok()
        .map(|d| serde_json::to_value(d.data()).expect("JSON にできる"));
    if copy.as_ref() == Some(&now) {
        (LinkStatus::Latest, "元のファイルと同じ".into())
    } else if link.pinned {
        (
            LinkStatus::Pinned,
            "元のファイルが変わりました(この版に固定しています)".into(),
        )
    } else {
        (
            LinkStatus::Stale,
            "元のファイルが変わりました(右クリックの「最新にする」で差し替えます)".into(),
        )
    }
}

/// 文書のリンクの様子。同じ(元のファイルの更新時刻、リンクのデータ)の間は、前に調べた結果を使う。
fn link_infos(
    cx: &Context<'_>,
    memo: &Mutex<HashMap<FeatureId, (u64, LinkStatus, String)>>,
) -> Vec<LinkInfo> {
    let mut memo = memo.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = Vec::new();
    for f in cx.data.features.iter().filter(|f| f.type_id == LINK) {
        let Ok(link) = serde_json::from_value::<Link>(f.data.clone()) else {
            continue;
        };
        let source = if link.broken {
            None
        } else {
            resolve(&link, cx.doc_path)
        };
        let mut h = DefaultHasher::new();
        f.data.to_string().hash(&mut h);
        source.hash(&mut h);
        source
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok()?.modified().ok())
            .hash(&mut h);
        let key = h.finish();
        let (status, note) = match memo.get(&f.id) {
            Some((k, s, n)) if *k == key => (*s, n.clone()),
            _ => {
                let src = match &source {
                    Some(p) => std::fs::read_to_string(p)
                        .map_err(|e| format!("{} を読めません: {e}", p.display())),
                    None => Err(format!("{} が見つかりません", link.path)),
                };
                let (s, n) = state(&link, src.as_deref().map_err(Clone::clone), cx.registry);
                log::info!("リンク {}「{}」の様子: {s:?}({n})", f.id.0, f.name);
                memo.insert(f.id, (key, s, n.clone()));
                (s, n)
            }
        };
        out.push(LinkInfo {
            feature: f.id,
            bodies: bodies_of(cx.data, f.id).into_iter().map(|b| b.0).collect(),
            source,
            status,
            note,
            // 今意味のある操作だけ(元と同じ・固定・元が無い時は最新にしない。元が無い時は選び直しを
            // 先に。切った後は何もしない)
            actions: match status {
                LinkStatus::Stale => vec![LINK_UPDATE, LINK_RELINK, LINK_PIN, LINK_BREAK],
                LinkStatus::Missing => vec![LINK_RELINK, LINK_PIN, LINK_BREAK],
                LinkStatus::Latest | LinkStatus::Pinned => vec![LINK_RELINK, LINK_PIN, LINK_BREAK],
                LinkStatus::Broken => vec![],
            }
            .into_iter()
            .map(String::from)
            .collect(),
        });
    }
    out
}
