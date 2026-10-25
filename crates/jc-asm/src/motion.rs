//! モーション スタディの操作。
//! 画面は窓を出し、窓のキーを置く・消すなどもこの操作で行う。CLI・MCP も同じ引数で使う。
//! 中身の型と補間は jc_interchange::motion(文書の区画 `asm.motion`)。

use crate::anim::drives;
use jc_core::{Context, FeatureId, Outcome};
use jc_interchange::motion::{self, MotionStudies, MotionStudy, STUDY_STEPS};

/// 操作の ID。
pub const MOTION_STUDY: &str = "asm.motion_study";

/// スタディを作る・変える。`study` の名前のスタディ(省くと最初のスタディ。無ければ作る)に対して:
/// `delete` で消す、`rename` で名前を変える、`seconds` で長さを変える、`joint` があれば
/// `drop` でスタディから外す、`remove` でステップ `step` のキーを消す、どちらも無ければステップ
/// `step`(省くと 0)にキー `value`(省くとジョイントの今の値)を置く。
pub fn run(cx: &Context<'_>) -> Result<Outcome, String> {
    let mut doc: MotionStudies = cx.data.section().map_err(|e| e.to_string())?;
    let len = doc.studies.len();
    let mut list = std::mem::take(&mut doc.studies);
    let name = match cx.arg_str("study").map(str::trim).filter(|s| !s.is_empty()) {
        Some(n) => n.to_string(),
        None => list
            .first()
            .map_or_else(|| motion::free_name(&list), |s| s.name.clone()),
    };
    let flag = |k: &str| cx.arg(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let at = match list.iter().position(|s| s.name == name) {
        Some(i) => i,
        None => {
            if flag("delete") {
                return Err(format!("スタディ「{name}」はありません"));
            }
            list.push(MotionStudy::new(name.clone()));
            list.len() - 1
        }
    };
    let created = at == len;
    let mut label = if created {
        format!("モーション スタディ「{name}」を作る")
    } else {
        format!("モーション スタディ「{name}」を変える")
    };
    if flag("delete") {
        list.remove(at);
        log::info!("モーション スタディ「{name}」を消す");
        return change(
            cx,
            doc,
            format!("モーション スタディ「{name}」を消す"),
            list,
        );
    }
    let study = &mut list[at];
    if let Some(n) = cx
        .arg_str("rename")
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        label = format!("モーション スタディ「{name}」の名前を「{n}」に変える");
        study.name = n.to_string();
    }
    if let Some(s) = cx.arg_f64("seconds")? {
        label = format!("モーション スタディ「{name}」の長さを {s} 秒にする");
        study.seconds = s;
    }
    if let Some(j) = cx.arg("joint").and_then(|v| v.as_u64()) {
        let joint = FeatureId(j);
        let (_, jname, d) = drives(cx)
            .into_iter()
            .find(|(id, _, _)| *id == joint)
            .ok_or_else(|| format!("機能 {j} は回転かスライダのジョイントではありません"))?;
        let step = match cx.arg_f64("step")? {
            None => 0,
            Some(s) if (0.0..=STUDY_STEPS as f64).contains(&s) && s.fract() == 0.0 => s as u32,
            Some(s) => {
                return Err(format!(
                    "ステップは 0〜{STUDY_STEPS} の整数にしてください(今 {s})"
                ));
            }
        };
        if flag("drop") {
            study.tracks.retain(|t| t.joint != joint);
            label = format!("{jname} をスタディ「{name}」から外す");
        } else if flag("remove") {
            if !study.track_mut(joint).remove_key(step) {
                return Err(format!("{jname} のステップ {step} にキーはありません"));
            }
            label = format!("{jname} のキーを消す(ステップ {step})");
        } else {
            let value = cx.arg_f64("value")?.unwrap_or_else(|| d.to_shown(d.value));
            study.track_mut(joint).set_key(step, value);
            label = format!("{jname} のキー(ステップ {step}: {value})");
        }
    } else if !created && cx.arg("rename").is_none() && cx.arg("seconds").is_none() {
        return Ok(Outcome::Message(format!(
            "モーション スタディ「{name}」はもうあります(変える物を引数で渡してください)"
        )));
    }
    log::info!("{label}");
    change(cx, doc, label, list)
}

/// 区画の残りの項目 `doc` はそのまま、スタディの並びを `list` にする変更。
fn change(
    cx: &Context<'_>,
    mut doc: MotionStudies,
    label: String,
    list: Vec<MotionStudy>,
) -> Result<Outcome, String> {
    doc.studies = list;
    Ok(Outcome::Change {
        label,
        edits: vec![cx.data.set_section(&doc)?],
    })
}
