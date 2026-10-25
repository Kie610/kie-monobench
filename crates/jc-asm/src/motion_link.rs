//! モーション リンク。2 つのジョイントの動きを
//! 比でつなぐ(例: バイスのハンドルが 1 回転すると、あごが 10 mm 動く)。
//!
//! つなぎは後ろのジョイントのデータに持つ(`drive_by: link`・`link_joint`・`link_lead`・
//! `link_follow`・`link_flip`。[`crate::Joint`])。後ろのジョイントは前のジョイントの今の値から自分の
//! 値を決めるので、前のジョイントの値を欄・引く操作で変えると再計算で一緒に動く。アニメーション・
//! モーション スタディ・動きを見る表示は、コアの [`jc_interchange::animation::Drive::lead`] で一緒に動かす。
//! 外すには、後ろのジョイントの「値の決め方」を「値で動かす」に戻す。

use crate::anim::drives;
use crate::joint::{DriveBy, JOINT, Joint, JointKind};
use jc_core::kernel as k;
use jc_core::{Context, Edit, FeatureId, Gesture, Handle, HandleChoice, Outcome};
use serde_json::json;

/// 操作の ID。
pub const MOTION_LINK: &str = "asm.motion_link";

/// 手順: つなぐジョイントの軸の線を 2 つクリック。
pub fn handles(cx: &Context<'_>) -> Vec<Handle> {
    let step = |key: &str, text: &str| {
        let mut h = Handle::new(key, text, Gesture::Choice);
        for (id, name, d) in drives(cx) {
            let half = k::scale(d.axis, 40.0);
            h.choices.push(HandleChoice {
                value: json!(id.0),
                label: format!("{name}({})", if d.slide { "スライダ" } else { "回転" }),
                outline: vec![vec![k::sub(d.origin, half), k::add(d.origin, half)]],
                group: 0,
                sized: false,
                color: Some([230, 140, 30]),
            });
        }
        h
    };
    vec![
        step(
            "joint",
            "1 つ目のジョイントの軸の線をクリック(回転かスライダ)",
        ),
        step("other", "2 つ目のジョイントの軸の線をクリック"),
    ]
}

/// 2 つのジョイント(`joint`・`other`)をつなぐ。タイムラインで後ろの方が前の方に連動する。
/// `lead`・`follow` は前の方と後ろの方の動きの量(式。省くと回転は 360 deg、スライダは 10 mm)、
/// `flip` で向きを逆にする。
pub fn run(cx: &Context<'_>) -> Result<Outcome, String> {
    let id = |key: &str, what: &str| {
        cx.arg(key)
            .and_then(|v| v.as_u64())
            .map(FeatureId)
            .ok_or(format!("{what}のジョイント({key})を選んでいません"))
    };
    let (a, b) = (id("joint", "1 つ目")?, id("other", "2 つ目")?);
    if a == b {
        return Err("同じジョイントです(別のジョイントを選んでください)".into());
    }
    let list = drives(cx);
    let find = |j: FeatureId| {
        list.iter().find(|(id, _, _)| *id == j).ok_or(format!(
            "機能 {} は回転かスライダのジョイントではありません",
            j.0
        ))
    };
    let (_, a_name, a_drive) = find(a)?;
    let (_, b_name, b_drive) = find(b)?;
    let at = |j: FeatureId| cx.data.index_of(j).unwrap_or(usize::MAX);
    let (lead, follow) = if at(a) < at(b) { (a, b) } else { (b, a) };
    let (lead_name, follow_name) = if lead == a {
        (a_name, b_name)
    } else {
        (b_name, a_name)
    };
    let amount = |slide: bool| if slide { "10 mm" } else { "360 deg" };
    let text = |key: &str, slide: bool| {
        cx.arg_str(key)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map_or_else(|| amount(slide).to_string(), str::to_string)
    };
    // 引数の順に打った量を、前と後ろに並べ直す
    let (a_amount, b_amount) = (text("lead", a_drive.slide), text("follow", b_drive.slide));
    let (lead_amount, follow_amount) = if lead == a {
        (a_amount, b_amount)
    } else {
        (b_amount, a_amount)
    };
    let entry = cx
        .data
        .feature(follow)
        .filter(|e| e.type_id == JOINT)
        .ok_or("ジョイントではありません")?;
    let mut j: Joint = serde_json::from_value(entry.data.clone()).map_err(|e| e.to_string())?;
    if !matches!(j.kind, JointKind::Revolute | JointKind::Slider) {
        return Err(format!(
            "{follow_name} は動きが 2 つ以上の種類なので連動できません(タイムラインで後ろのジョイントが連動します。回転かスライダにしてください)"
        ));
    }
    if let Some(prev) = cx.data.feature(lead).and_then(|e| {
        serde_json::from_value::<Joint>(e.data.clone())
            .ok()
            .and_then(|l| l.linked())
    }) && prev == follow
    {
        return Err(format!("{lead_name} はもう {follow_name} に連動しています"));
    }
    j.drive_by = DriveBy::Link;
    j.link_joint = Some(lead);
    j.link_lead = lead_amount.clone();
    j.link_follow = follow_amount.clone();
    j.link_flip = cx.arg("flip").and_then(|v| v.as_bool()).unwrap_or(false);
    let label = format!("{follow_name} を {lead_name} に連動({lead_amount} で {follow_amount})");
    log::info!("{label}");
    Ok(Outcome::Change {
        label,
        edits: vec![Edit::SetData {
            id: follow,
            data: serde_json::to_value(&j).map_err(|e| e.to_string())?,
        }],
    })
}
