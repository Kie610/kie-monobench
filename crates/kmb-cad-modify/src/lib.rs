//! ボディと面を直す機能: ボディの分割([`split`])、ボディの移動・コピー([`move_body`])、
//! 面のオフセット([`offset`])、ボディの尺度([`scale`])、勾配・面を置換・面を削除([`face_edit`])。どれもタイムラインに機能として残る(履歴の外で形を変えない)。
mod face_cell;
pub mod face_edit;
pub mod move_body;
pub mod offset;
pub mod scale;
pub mod split;

pub use face_edit::{
    DELETE_FACES, DRAFT, DRAFT_PLANE, DeleteFaces, Draft, REPLACE_FACE, ReplaceFace,
};
pub use move_body::{MOVE, MOVE_AXIS, MoveBody, MoveMode};
pub use offset::{OFFSET_FACES, OffsetFaces};
pub use scale::{SCALE, ScaleBody, ScaleMode};
pub use split::{SPLIT, SPLIT_PART, SPLIT_PLANE, Split, SplitPart};

use kmb_cad::AxisRef;
use kmb_cad::plane::PlaneRef;
use kmb_core::{Action, Context, Extension, FeatureId, Gesture, Handle, Outcome, Place, Registry};
use serde_json::json;

pub fn register(reg: &mut Registry) {
    split::register(reg);
    move_body::register(reg);
    offset::register(reg);
    scale::register(reg);
    face_edit::register(reg);
}

/// 選んでいる機能が種類 `type_id` なら、その番号とデータ。
fn selected(cx: &Context<'_>, type_id: &str) -> Option<(FeatureId, serde_json::Value)> {
    let id = cx.selected?;
    let entry = cx.data.feature(id)?;
    (entry.type_id == type_id).then(|| (id, entry.data.clone()))
}

/// 割る平面を、手順で選んだ値(`plane` が xy・yz・zx か face)にする。
fn split_plane(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, mut data) = selected(cx, SPLIT).ok_or("ボディの分割を選んでいません")?;
    let plane = PlaneRef::from_choice(cx, id, "割る平面")?;
    data["plane_ref"] = serde_json::to_value(plane).map_err(|e| e.to_string())?;
    Ok(Outcome::Change {
        label: "割る平面を選ぶ".into(),
        edits: vec![kmb_core::Edit::SetData { id, data }],
    })
}

/// 勾配の引き出し方向の平面を、手順で選んだ値(`plane` が xy・yz・zx か face)にする。
fn pull_plane(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, mut data) = selected(cx, DRAFT).ok_or("勾配を選んでいません")?;
    let plane = PlaneRef::from_choice(cx, id, "引き出し方向の平面")?;
    data["plane_ref"] = serde_json::to_value(plane).map_err(|e| e.to_string())?;
    Ok(Outcome::Change {
        label: "引き出し方向を選ぶ".into(),
        edits: vec![kmb_core::Edit::SetData { id, data }],
    })
}

/// 回転の軸を、手順で選んだ値(`axis`)にする。
fn move_axis(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, mut data) = selected(cx, MOVE).ok_or("移動/コピーを選んでいません")?;
    let v = cx
        .arg("axis")
        .ok_or("axis に軸(x・y・z か {\"sketch\": スケッチ, \"line\": 線の番号})を指定します")?;
    let axis = AxisRef::from_choice(v, cx)?;
    data["axis_ref"] = serde_json::to_value(axis).map_err(|e| e.to_string())?;
    data["mode"] = json!("rotate");
    Ok(Outcome::Change {
        label: "回転の軸を選ぶ".into(),
        edits: vec![kmb_core::Edit::SetData { id, data }],
    })
}

/// 割る平面と回転の軸を選ぶ操作。
pub struct ModifyCommands;

impl Extension for ModifyCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let split = selected(cx, SPLIT).is_some();
        let moving = selected(cx, MOVE).is_some();
        let draft = selected(cx, DRAFT).is_some();
        vec![
            Action::new(SPLIT_PLANE, "割る平面を選ぶ", "cad")
                .args(kmb_cad::plane::args())
                .menu("修正")
                .icon("split")
                .aliases("ぶんかつ わる へいめん 平面 めん split plane")
                .hint("選んでいるボディの分割の平面を、原点の XY・YZ・ZX 平面の四角か、ボディの平らな面をクリックして決める")
                .keep_edit()
                .at(if split {
                    &[Place::FeatureMenu, Place::Panel]
                } else {
                    &[]
                })
                .disabled_if((!split).then_some("ボディの分割を選んでから使う")),
            Action::new(MOVE_AXIS, "回転の軸を選ぶ", "cad")
                .arg(
                    kmb_core::Arg::value(
                        "axis",
                        "軸(x・y・z か {\"sketch\": スケッチ, \"line\": 線の番号}、手順の値)",
                    )
                    .required(),
                )
                .menu("修正")
                .icon("movecopy")
                .aliases("いどう かいてん じく 軸 move rotate axis 線 原点")
                .hint("選んでいる移動/コピーの回転の軸を、原点の X・Y・Z 軸・スケッチの線・ボディの辺(円の辺はその円の軸)をクリックして決める")
                .keep_edit()
                .at(if moving {
                    &[Place::FeatureMenu, Place::Panel]
                } else {
                    &[]
                })
                .disabled_if((!moving).then_some("移動/コピーを選んでから使う")),
            Action::new(DRAFT_PLANE, "引き出し方向を選ぶ", "cad")
                .args(kmb_cad::plane::args())
                .menu("修正")
                .icon("draft")
                .aliases("こうばい ひきだし 引き出し方向 へいめん 平面 draft pull plane")
                .hint("選んでいる勾配の引き出し方向の平面を、原点の XY・YZ・ZX 平面の四角か、ボディの平らな面をクリックして決める")
                .keep_edit()
                .at(if draft {
                    &[Place::FeatureMenu, Place::Panel]
                } else {
                    &[]
                })
                .disabled_if((!draft).then_some("勾配を選んでから使う")),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        Some(match id {
            SPLIT_PLANE => split_plane(cx),
            MOVE_AXIS => move_axis(cx),
            DRAFT_PLANE => pull_plane(cx),
            _ => return None,
        })
    }

    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        match id {
            SPLIT_PLANE => {
                selected(cx, SPLIT)?;
                // ミラーの対称の平面と同じく、原点の平面の四角とボディの平らな面をクリックで選べる
                Some(vec![Handle {
                    choices: ["xy", "yz", "zx"]
                        .iter()
                        .enumerate()
                        .map(|(i, v)| kmb_core::HandleChoice::origin_plane(i, json!(v)))
                        .collect(),
                    faces: true,
                    ..Handle::new(
                        "plane",
                        "割る平面にする原点の平面(XY・YZ・ZX の四角)か、ボディの平らな面をクリック",
                        Gesture::Choice,
                    )
                }])
            }
            DRAFT_PLANE => {
                selected(cx, DRAFT)?;
                Some(vec![Handle {
                    choices: ["xy", "yz", "zx"]
                        .iter()
                        .enumerate()
                        .map(|(i, v)| kmb_core::HandleChoice::origin_plane(i, json!(v)))
                        .collect(),
                    faces: true,
                    ..Handle::new(
                        "plane",
                        "引き出し方向にする原点の平面(XY・YZ・ZX の四角)か、ボディの平らな面をクリック",
                        Gesture::Choice,
                    )
                }])
            }
            MOVE_AXIS => {
                let (mid, _) = selected(cx, MOVE)?;
                Some(vec![Handle {
                    choices: kmb_cad::axis::axis_choices(cx, mid, None, None),
                    show_origin: true,
                    ..Handle::new(
                        "axis",
                        "回転の軸にする原点の軸・スケッチの線・ボディの辺をクリック",
                        Gesture::Choice,
                    )
                }])
            }
            _ => None,
        }
    }
}

/// この crate の名乗り(登録する種類と操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cad-modify", register).with_extension(ModifyCommands)
}
