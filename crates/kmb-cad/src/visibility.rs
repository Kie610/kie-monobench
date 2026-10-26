//! 操作「表示/非表示」(目の開け閉め)。中身の型は kmb_interchange::visibility(文書の区画
//! `cad.visibility`)。画面の目・右クリック・キーと、CLI・MCP・コマンド欄が同じ宣言と確かめを通る。
//! 変更は区画を置き換える 1 つで、取り消しは前の区画に戻す逆の変更。形は計算し直さない。

use kmb_core::{Action, Arg, Context, Extension, FeatureId, Outcome, Problem};
use kmb_interchange::visibility::{self as vis, Folder, Visibility};

/// 目を開け閉めする操作。
pub const SET_VISIBILITY: &str = "cad.visibility";

/// 表示/非表示の操作。
pub struct VisibilityCommands;

/// 目を替える物 1 つ。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    /// 行の目(ボディは最初に作った機能)
    Feature(FeatureId),
    Component(u64),
    Folder(Folder),
    Origin,
}

impl Target {
    fn shown(self, v: &Visibility) -> bool {
        match self {
            Target::Feature(id) => !v.hidden.contains(&id),
            Target::Component(id) => !v.hidden_components.contains(&id),
            Target::Folder(f) => !v.hidden_folders.contains(&f),
            Target::Origin => v.origin_shown,
        }
    }

    fn set(self, v: &mut Visibility, show: bool) {
        fn put<T: Ord>(set: &mut std::collections::BTreeSet<T>, item: T, hide: bool) {
            if hide {
                set.insert(item);
            } else {
                set.remove(&item);
            }
        }
        match self {
            Target::Feature(id) => put(&mut v.hidden, id, !show),
            Target::Component(id) => put(&mut v.hidden_components, id, !show),
            Target::Folder(f) => put(&mut v.hidden_folders, f, !show),
            Target::Origin => v.origin_shown = show,
        }
    }

    fn label(self, cx: &Context<'_>) -> String {
        match self {
            Target::Feature(id) => match vis::folder_of(cx, id) {
                Some(Folder::Bodies) => kmb_core::body_labels(cx.data, cx.registry, &[id])
                    .pop()
                    .unwrap_or_default(),
                _ => cx
                    .data
                    .feature(id)
                    .map_or_else(String::new, |f| f.name.clone()),
            },
            Target::Component(id) => cx
                .data
                .components
                .iter()
                .find(|c| c.id == id)
                .map_or_else(String::new, |c| c.name.clone()),
            Target::Folder(f) => f.label().into(),
            Target::Origin => "原点".into(),
        }
    }
}

/// 番号の並びの引数 `key`(整数でない物は誤り)。
fn numbers(cx: &Context<'_>, key: &str) -> Result<Vec<u64>, String> {
    let Some(list) = cx.arg(key).and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    list.iter()
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| format!("{key} は番号の並びです: {v}"))
        })
        .collect()
}

/// 引数から目を替える物を集める。機能を省くと選んでいる機能。
fn targets(cx: &Context<'_>) -> Result<Vec<Target>, String> {
    let mut features: Vec<FeatureId> = numbers(cx, "features")?
        .into_iter()
        .map(FeatureId)
        .collect();
    let components = numbers(cx, "components")?;
    let folder = cx.arg_str("folder");
    let origin = cx.arg("origin").and_then(|v| v.as_bool()) == Some(true);
    if cx.arg("features").is_none() && components.is_empty() && folder.is_none() && !origin {
        features.extend(cx.selected);
        features.extend(cx.also_selected.iter().copied());
    }
    let mut out: Vec<Target> = Vec::new();
    let mut push = |t: Target| {
        if !out.contains(&t) {
            out.push(t);
        }
    };
    for id in features {
        if cx.data.feature(id).is_none() {
            return Err(format!("機能 {} がありません", id.0));
        }
        push(Target::Feature(vis::eye_of(cx, id)));
    }
    for id in components {
        if !cx.data.components.iter().any(|c| c.id == id) {
            return Err(format!("部品 {id} がありません"));
        }
        push(Target::Component(id));
    }
    if let Some(key) = folder {
        let f = Folder::from_key(key).ok_or_else(|| format!("まとまり `{key}` が分かりません"))?;
        push(Target::Folder(f));
    }
    if origin {
        push(Target::Origin);
    }
    if out.is_empty() {
        return Err("表示/非表示にする物を選んでいません".into());
    }
    Ok(out)
}

fn run(cx: &Context<'_>) -> Result<Outcome, Problem> {
    let targets = targets(cx)?;
    let mut v = vis::of(cx);
    // 省くと、1 つでも見えていれば全部隠し、全部隠れていれば全部出す
    let show = match cx.arg("show").and_then(|s| s.as_bool()) {
        Some(s) => s,
        None => !targets.iter().any(|t| t.shown(&v)),
    };
    let before = v.clone();
    for t in &targets {
        t.set(&mut v, show);
    }
    // 区画の無い文書は、目を替えなくても区画を書く(見え方を保存する)ので、区画があって中が同じ時だけ止める
    if v == before
        && cx
            .data
            .sections
            .contains_key(<Visibility as kmb_core::SectionType>::KEY)
    {
        return Ok(Outcome::Message(if show {
            "もう表示しています".into()
        } else {
            "もう非表示です".into()
        }));
    }
    let what = match targets.as_slice() {
        [one] => format!("「{}」", one.label(cx)),
        many => format!("{} 個", many.len()),
    };
    let label = format!("{what}を{}", if show { "表示" } else { "非表示" });
    log::info!("{label}");
    Ok(Outcome::Change {
        label,
        edits: vec![cx.data.set_section(&v)?],
    })
}

impl Extension for VisibilityCommands {
    fn actions(&self, _cx: &Context<'_>) -> Vec<Action> {
        // 画面はブラウザの目・右クリック・キーから呼ぶので、ツールバーには出さない(コマンド検索には出る)
        let folders: Vec<(&str, &str)> = Folder::ALL.iter().map(|f| (f.key(), f.label())).collect();
        vec![
            Action::new(SET_VISIBILITY, "表示/非表示", "cad")
                .role(kmb_interchange::role::action::SET_VISIBILITY)
                .args([
                    Arg::list(
                        "features",
                        "目を替える機能の番号の並び(ボディはどの機能でもよい。省くと選んでいる機能)",
                    ),
                    Arg::list("components", "目を替える部品の番号の並び"),
                    Arg::choice("folder", "目を替えるまとまり", &folders),
                    Arg::flag("origin", "原点の目を替える"),
                    Arg::flag(
                        "show",
                        "見せるか(省くと、1 つでも見えていれば全部隠し、全部隠れていれば全部出す)",
                    ),
                ])
                .aliases("ひょうじ 表示 ひひょうじ 非表示 かくす 隠す みせる 見せる め 目 visibility hide show")
                .hint("ボディ・スケッチ・作業平面・部品・まとまり・原点の目を開け閉めする(文書に保存し、取り消せる。形は計算し直さない)"),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, Problem>> {
        (id == SET_VISIBILITY).then(|| run(cx))
    }
}
