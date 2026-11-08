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
pub use move_body::{
    EndPoint, MOVE, MOVE_AXIS, MOVE_FROM, MOVE_POINTS, MOVE_TO, MoveBody, MoveMode,
};
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

/// 割る平面を、手順で選んだ値(`plane` が xy・yz・zx・face か作業平面)にする。
fn split_plane(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let (id, mut data) = selected(cx, SPLIT).ok_or("ボディの分割を選んでいません")?;
    let plane = PlaneRef::from_choice(cx, id, "割る平面")?;
    data["plane_ref"] = serde_json::to_value(plane).map_err(|e| e.to_string())?;
    Ok(Outcome::Change {
        label: "割る平面を選ぶ".into(),
        edits: vec![kmb_core::Edit::SetData { id, data }],
    })
}

/// 勾配の引き出し方向の平面を、手順で選んだ値(`plane` が xy・yz・zx・face か作業平面)にする。
fn pull_plane(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let (id, mut data) = selected(cx, DRAFT).ok_or("勾配を選んでいません")?;
    let plane = PlaneRef::from_choice(cx, id, "引き出し方向の平面")?;
    data["plane_ref"] = serde_json::to_value(plane).map_err(|e| e.to_string())?;
    Ok(Outcome::Change {
        label: "引き出し方向を選ぶ".into(),
        edits: vec![kmb_core::Edit::SetData { id, data }],
    })
}

/// 回転の軸を、手順で選んだ値(`axis`)にする。
fn move_axis(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
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

/// 点から点の端の点を、手順で選んだ値にする。`ends` は(引数の鍵、データの鍵、点の説明)。
/// 値はこの移動より前にある点の候補([`kmb_cad::point_choices`])のどれかでなければ誤り。
fn move_points(
    cx: &Context<'_>,
    ends: &[(&str, &str, &str)],
) -> Result<Outcome, kmb_core::Problem> {
    let (id, mut data) = selected(cx, MOVE).ok_or("移動/コピーを選んでいません")?;
    let choices: Vec<kmb_cad::PointRef> = kmb_cad::point_choices(cx, Some(id))
        .into_iter()
        .filter_map(|c| serde_json::from_value(c.value).ok())
        .collect();
    let mut set = Vec::new();
    for (arg, key, what) in ends {
        let Some(v) = cx.arg(arg).filter(|v| !v.is_null()) else {
            continue;
        };
        let point: kmb_cad::PointRef = serde_json::from_value(v.clone())
            .map_err(|e| format!("{what}の参照を読めません: {e}"))?;
        if !choices.contains(&point) {
            return Err(format!(
                "{what}にした点が、この移動より前の見えている形・スケッチ・作業点にありません"
            )
            .into());
        }
        data[*key] = serde_json::to_value(point).map_err(|e| e.to_string())?;
        set.push(*what);
    }
    if set.is_empty() {
        let args: Vec<&str> = ends.iter().map(|e| e.0).collect();
        return Err(format!(
            "{} に点({{\"kind\": \"vertex\", \"feature\": ボディ, \"vertex\": 角の名前}} など、手順の値)を指定します",
            args.join("・")
        )
        .into());
    }
    data["mode"] = json!("point_to_point");
    Ok(Outcome::Change {
        label: format!("{}を選ぶ", set.join("と")),
        edits: vec![kmb_core::Edit::SetData { id, data }],
    })
}

const FROM: (&str, &str, &str) = ("from", "from_ref", "元の点");
const TO: (&str, &str, &str) = ("to", "to_ref", "先の点");

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
                .hint("選んでいるボディの分割の平面を、原点の XY・YZ・ZX 平面の四角か作業平面か、ボディの平らな面をクリックして決める")
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
            point_action(MOVE_POINTS, "元の点と先の点を選ぶ", "元の点と先の点", &[FROM, TO], moving),
            point_action(MOVE_FROM, "元の点を選ぶ", "動かすボディの上の点", &[FROM], moving),
            point_action(MOVE_TO, "先の点を選ぶ", "元の点が来る所", &[TO], moving),
            Action::new(DRAFT_PLANE, "引き出し方向を選ぶ", "cad")
                .args(kmb_cad::plane::args())
                .menu("修正")
                .icon("draft")
                .aliases("こうばい ひきだし 引き出し方向 へいめん 平面 draft pull plane")
                .hint("選んでいる勾配の引き出し方向の平面を、原点の XY・YZ・ZX 平面の四角か作業平面か、ボディの平らな面をクリックして決める")
                .keep_edit()
                .at(if draft {
                    &[Place::FeatureMenu, Place::Panel]
                } else {
                    &[]
                })
                .disabled_if((!draft).then_some("勾配を選んでから使う")),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        Some(match id {
            SPLIT_PLANE => split_plane(cx),
            MOVE_AXIS => move_axis(cx),
            MOVE_POINTS => move_points(cx, &[FROM, TO]),
            MOVE_FROM => move_points(cx, &[FROM]),
            MOVE_TO => move_points(cx, &[TO]),
            DRAFT_PLANE => pull_plane(cx),
            _ => return None,
        })
    }

    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        match id {
            SPLIT_PLANE => {
                let (sid, _) = selected(cx, SPLIT)?;
                // ミラーの対称の平面と同じく、原点の平面・作業平面の四角とボディの平らな面を
                // クリックで選べる
                Some(vec![Handle {
                    choices: kmb_cad::plane::plane_choices(cx, sid),
                    faces: true,
                    ..Handle::new(
                        "plane",
                        "割る平面にする原点の平面(XY・YZ・ZX の四角)か作業平面か、ボディの平らな面をクリック",
                        Gesture::Choice,
                    )
                }])
            }
            DRAFT_PLANE => {
                let (did, _) = selected(cx, DRAFT)?;
                Some(vec![Handle {
                    choices: kmb_cad::plane::plane_choices(cx, did),
                    faces: true,
                    ..Handle::new(
                        "plane",
                        "引き出し方向にする原点の平面(XY・YZ・ZX の四角)か作業平面か、ボディの平らな面をクリック",
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
            MOVE_POINTS | MOVE_FROM | MOVE_TO => {
                let (mid, _) = selected(cx, MOVE)?;
                let choices = kmb_cad::point_choices(cx, Some(mid));
                let ends: &[(&str, &str, &str)] = match id {
                    MOVE_POINTS => &[FROM, TO],
                    MOVE_FROM => &[FROM],
                    _ => &[TO],
                };
                Some(
                    ends.iter()
                        .map(|(arg, _, what)| Handle {
                            choices: choices.clone(),
                            ..Handle::new(
                                arg,
                                &format!(
                                    "{what}にするボディの角・円の中心・辺の中点・スケッチの点・作業点をクリック"
                                ),
                                Gesture::Choice,
                            )
                        })
                        .collect(),
                )
            }
            _ => None,
        }
    }
}

/// 点から点の端の点を選ぶ操作(`what` は点の説明、`ends` は受ける端)。
fn point_action(
    id: &str,
    label: &str,
    what: &str,
    ends: &[(&str, &str, &str)],
    moving: bool,
) -> Action {
    let mut a = Action::new(id, label, "cad");
    for (arg, _, end) in ends {
        let help = format!(
            "{end}の参照({{\"kind\": \"vertex\", \"feature\": ボディ, \"vertex\": 角の名前}}・center・midpoint(faces に辺の両側の面)・sketch・datum。手順の値)"
        );
        let arg = kmb_core::Arg::value(arg, &help);
        a = a.arg(if ends.len() == 1 { arg.required() } else { arg });
    }
    a
        .menu("修正")
        .icon("movecopy")
        .aliases("いどう てんからてん 点から点 てん 点 かど 角 ちゅうてん 中点 ちゅうしん 中心 move point")
        .hint(&format!(
            "選んでいる移動/コピーの点から点の{what}を、ボディの角・円の中心・辺の中点・スケッチの点・作業点をクリックして決める。元の形を変えても同じ点を追う"
        ))
        .keep_edit()
        // 編集パネルには続けて選ぶ物だけを出し、片方の選び直しは右クリックに置く(ボタンを増やしすぎない)
        .at(match (moving, ends.len()) {
            (false, _) => &[],
            (true, 1) => &[Place::FeatureMenu],
            (true, _) => &[Place::FeatureMenu, Place::Panel],
        })
        .disabled_if((!moving).then_some("移動/コピーを選んでから使う"))
}

/// この crate の名乗り(登録する種類と操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cad-modify", register).with_extension(ModifyCommands)
}
