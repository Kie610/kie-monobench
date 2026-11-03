use crate::*;
use kmb_core::{Action, Arg, Context, Extension, Outcome, Problem};
use serde::de::DeserializeOwned;

pub struct DrawingCommands;
const ACTIONS: [(&str, &str, &str); 9] = [
    ("drawing.add_sheet", "用紙を追加", "図面に用紙を追加する"),
    (
        "drawing.base_view",
        "基準ビュー",
        "形と名前の向きから基準ビューを作る",
    ),
    (
        "drawing.projected_view",
        "投影ビュー",
        "親のビューから投影ビューを作る",
    ),
    (
        "drawing.section_view",
        "断面ビュー",
        "親のビューの切断線から断面ビューを作る",
    ),
    (
        "drawing.detail_view",
        "詳細ビュー",
        "親のビューの円の内側を拡大する",
    ),
    (
        "drawing.move_view",
        "ビューを移動",
        "用紙の上の位置か親からの距離を変える",
    ),
    (
        "drawing.set_view",
        "ビューを編集",
        "ビューの尺度と隠れ線や接線の表示を変える",
    ),
    (
        "drawing.delete",
        "図面を削除",
        "用紙・ビュー・注記を削除する。子のビューがある時は先に子を削除する",
    ),
    (
        "drawing.picture",
        "図面の線を書き出す",
        "用紙の線と問題を JSON に書き出す",
    ),
];
fn args(id: &str) -> Vec<Arg> {
    if id == "drawing.add_sheet" {
        return vec![
            Arg::text("id", "用紙の id").required(),
            Arg::text("name", "用紙の名前").required(),
            Arg::value("paper", "用紙の形式と縦向きの指定"),
            Arg::value(
                "projection",
                "第三角法 third_angle または第一角法 first_angle",
            ),
        ];
    }
    let mut a = vec![Arg::text("sheet", "用紙の id").required()];
    if id == "drawing.picture" {
        return a;
    }
    a.push(Arg::text("id", "ビュー・注記の id。削除は用紙の id も指定できる").required());
    match id {
        "drawing.base_view"
        | "drawing.projected_view"
        | "drawing.section_view"
        | "drawing.detail_view" => {
            a.extend([
                Arg::text("label", "ビューの名前"),
                Arg::value(
                    "scale",
                    "尺度 {num, den}。詳細ビューの作成時は detail_scale で指定する",
                ),
                Arg::value(
                    "at",
                    "用紙の mm の位置 [x,y]。投影・断面は [親からの距離,0]",
                ),
                Arg::value("options", "hidden と tangent の表示指定"),
            ]);
            match id {
                "drawing.base_view" => a.extend([
                    Arg::value("source", "all・component・bodies の形の参照"),
                    Arg::value("orientation", "front・top・right・left・bottom・back・iso")
                        .required(),
                    Arg::value("rotate", "面内の回転 0・90・180・270 度"),
                ]),
                "drawing.projected_view" => a.extend([
                    Arg::text("parent", "親のビューの id").required(),
                    Arg::value("side", "top・bottom・left・right と斜めの側").required(),
                ]),
                "drawing.section_view" => a.extend([
                    Arg::text("parent", "親のビューの id").required(),
                    Arg::value("line", "親のビューの mm の切断線 [[x,y],[x,y]]").required(),
                    Arg::text("letter", "断面の文字。英大文字").required(),
                    Arg::value("uncut", "切らないボディの id の並び"),
                ]),
                _ => a.extend([
                    Arg::text("parent", "親のビューの id").required(),
                    Arg::value("center", "親のビューの mm の中心 [x,y]").required(),
                    Arg::length("radius", "詳細の円の半径。親のビューの mm").required(),
                    Arg::value("detail_scale", "詳細ビューの尺度 {num,den}").required(),
                    Arg::text("letter", "詳細の文字。英大文字").required(),
                ]),
            }
        }
        "drawing.move_view" => a.push(
            Arg::value(
                "at",
                "用紙の mm の位置 [x,y]。投影・断面は [親からの距離,0]",
            )
            .required(),
        ),
        "drawing.set_view" => a.extend([
            Arg::value("scale", "尺度 {num,den}。詳細ビューは詳細の尺度を変える"),
            Arg::value("options", "hidden と tangent の表示指定"),
        ]),
        _ => {}
    }
    a
}
pub(super) fn read<T: DeserializeOwned>(cx: &Context<'_>, key: &str) -> Result<T, Problem> {
    serde_json::from_value(
        cx.arg(key)
            .ok_or_else(|| format!("{key} を指定してください"))?
            .clone(),
    )
    .map_err(|e| format!("{key}: {e}").into())
}
pub(super) fn optional<T: DeserializeOwned + Default>(
    cx: &Context<'_>,
    key: &str,
) -> Result<T, Problem> {
    if cx.arg(key).is_some() {
        read(cx, key)
    } else {
        Ok(T::default())
    }
}
fn run(id: &str, cx: &Context<'_>) -> Result<Outcome, Problem> {
    let mut drawings: Drawings = cx.data.section().map_err(|e| e.to_string())?;
    if id == "drawing.picture" {
        let sheet: String = read(cx, "sheet")?;
        let (picture, lines) = sheet_picture_with_lines(cx, &sheet)?;
        return Ok(Outcome::File {
            suffix: "図面.json".into(),
            contents: serde_json::to_vec_pretty(&picture).map_err(|e| e.to_string())?,
            info: serde_json::json!({"items":picture.items.len(),"problems":picture.problems.len(),"references":lines}),
        });
    }
    if id == "drawing.add_sheet" {
        let mut sheet = Sheet::new(read::<String>(cx, "id")?, read::<String>(cx, "name")?);
        sheet.paper = optional(cx, "paper")?;
        sheet.projection = optional(cx, "projection")?;
        drawings.sheets.push(sheet);
    } else {
        let sheet_id: String = read(cx, "sheet")?;
        let target: String = read(cx, "id")?;
        if id == "drawing.delete" && target == sheet_id {
            let n = drawings.sheets.len();
            drawings.sheets.retain(|s| s.id != sheet_id);
            if drawings.sheets.len() == n {
                return Err("用紙がありません".into());
            }
        } else {
            let sheet = drawings
                .sheets
                .iter_mut()
                .find(|s| s.id == sheet_id)
                .ok_or("用紙がありません")?;
            match id {
                "drawing.move_view" | "drawing.set_view" => {
                    let v = sheet
                        .views
                        .iter_mut()
                        .find(|v| v.id == target)
                        .ok_or("ビューがありません")?;
                    if id == "drawing.move_view" {
                        v.at = read(cx, "at")?;
                    } else {
                        if cx.arg("scale").is_none() && cx.arg("options").is_none() {
                            return Err("尺度か表示の欄を指定してください".into());
                        }
                        if cx.arg("scale").is_some() {
                            let scale: Ratio = read(cx, "scale")?;
                            if let ViewKind::Detail { scale: s, .. } = &mut v.kind {
                                *s = scale;
                            } else {
                                v.scale = scale;
                            }
                        }
                        if cx.arg("options").is_some() {
                            v.options = read(cx, "options")?;
                        }
                    }
                }
                "drawing.delete" => {
                    if sheet
                        .views
                        .iter()
                        .any(|v| v.parent() == Some(target.as_str()))
                    {
                        return Err("子のビューを先に削除してください".into());
                    }
                    let note_count = sheet.notes.len();
                    sheet
                        .notes
                        .retain(|n| n.get("id").and_then(|v| v.as_str()) != Some(&target));
                    if sheet.notes.len() == note_count {
                        let n = sheet.views.len();
                        sheet.views.retain(|v| v.id != target);
                        if sheet.views.len() == n {
                            return Err("ビューか注記がありません".into());
                        }
                    }
                }
                _ => {
                    let kind = match id {
                        "drawing.base_view" => ViewKind::Base {
                            source: optional(cx, "source")?,
                            orientation: read(cx, "orientation")?,
                            rotate: optional(cx, "rotate")?,
                        },
                        "drawing.projected_view" => ViewKind::Projected {
                            parent: read(cx, "parent")?,
                            side: read(cx, "side")?,
                        },
                        "drawing.section_view" => ViewKind::Section {
                            parent: read(cx, "parent")?,
                            line: read(cx, "line")?,
                            letter: read(cx, "letter")?,
                            uncut: optional(cx, "uncut")?,
                        },
                        "drawing.detail_view" => ViewKind::Detail {
                            parent: read(cx, "parent")?,
                            center: read(cx, "center")?,
                            radius: read(cx, "radius")?,
                            scale: read(cx, "detail_scale")?,
                            letter: read(cx, "letter")?,
                        },
                        _ => return Err("知らない図面の操作です".into()),
                    };
                    sheet.views.push(DrawingView {
                        id: target,
                        label: optional(cx, "label")?,
                        kind,
                        scale: optional(cx, "scale")?,
                        at: optional(cx, "at")?,
                        options: optional(cx, "options")?,
                        rest: Rest::new(),
                    });
                }
            }
        }
    }
    let edit = cx.data.set_section(&drawings)?;
    log::info!("図面の操作 {id}: 用紙 {} 枚", drawings.sheets.len());
    Ok(Outcome::Change {
        label: ACTIONS.iter().find(|a| a.0 == id).unwrap().1.into(),
        edits: vec![edit],
    })
}
impl Extension for DrawingCommands {
    fn actions(&self, _cx: &Context<'_>) -> Vec<Action> {
        ACTIONS
            .iter()
            .map(|(id, name, hint)| {
                Action::new(id, name, "cad")
                    .hint(hint)
                    .at(&[])
                    .args(args(id))
            })
            .collect()
    }
    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, Problem>> {
        ACTIONS.iter().any(|a| a.0 == id).then(|| run(id, cx))
    }
}
