//! 用紙・表題欄と、同じ用紙の線を返す書き出しの操作。

use crate::*;
use kmb_core::{Action, Arg, Context, Extension, Outcome, Problem};
use serde::de::DeserializeOwned;

pub(super) struct ExportCommands;
const ACTIONS: &[(&str, &str, &str)] = &[
    (
        "drawing.set_paper",
        "用紙を設定",
        "図面の用紙の大きさ・向き・とじ代・ペンと画層の色を決める",
    ),
    (
        "drawing.set_title",
        "表題欄を設定",
        "表題欄の固定の文字や文書から取る項目と、使う文字の書体を設定する",
    ),
    (
        "drawing.export_pdf",
        "図面をPDFに書き出す",
        "選んだ用紙か全部の用紙を、実寸のページのPDFに書き出す",
    ),
    (
        "drawing.export_dxf",
        "図面をDXFに書き出す",
        "選んだ用紙を、線の役ごとの画層と線種を持つDXFに書き出す",
    ),
    (
        "drawing.export_svg",
        "図面をSVGに書き出す",
        "選んだ用紙を、用紙の大きさと線の太さと線種を持つSVGに書き出す",
    ),
];
fn read<T: DeserializeOwned>(cx: &Context<'_>, key: &str) -> Result<T, Problem> {
    serde_json::from_value(
        cx.arg(key)
            .ok_or_else(|| format!("{key} を指定してください"))?
            .clone(),
    )
    .map_err(|e| format!("{key}: {e}").into())
}
fn args(id: &str) -> Vec<Arg> {
    let sheet = Arg::text("sheet", "用紙のid。PDFは省くと全部の用紙");
    let mut args = vec![if id == "drawing.export_pdf" {
        sheet
    } else {
        sheet.required()
    }];
    match id {
        "drawing.set_paper"=>args.extend([Arg::value("paper","format・portrait・binding・colors のうち変更する欄"),Arg::value("pens","thick・thin・extra_thick のうち変更する太さ(mm)")]),
        "drawing.set_title"=>args.extend([Arg::value("fields","title・number・scale・projection・date・designer・material・sheet・unit。それぞれ固定の文字か {source:document|part_number|scale|projection|date|designer|material|sheet|unit}").required(),Arg::file("font","日本語の表題を文書に埋める書体のファイル。字形が既にあれば省略できる"),Arg::value("face","書体のファイルの中の番号。既定0").default(0)]),
        _=>args.push(Arg::text("date","書き出しの日付 YYYY-MM-DD。省くと日付は空で、時計は読まない")),
    }
    args
}
fn merge(
    value: &mut serde_json::Value,
    patch: &serde_json::Value,
    allowed: &[&str],
) -> Result<(), String> {
    let patch = patch
        .as_object()
        .ok_or("変更する欄の名前と値で指定してください")?;
    if let Some(key) = patch.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(format!("欄 {key} はありません"));
    }
    let target = value.as_object_mut().ok_or("保存した欄を読めません")?;
    for (key, value) in patch {
        if key == "colors"
            && let Some(before) = target.get_mut(key).and_then(|v| v.as_object_mut())
            && let Some(after) = value.as_object()
        {
            before.extend(after.iter().map(|(k, v)| (k.clone(), v.clone())));
        } else {
            target.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}
fn run(id: &str, cx: &Context<'_>) -> Result<Outcome, Problem> {
    let mut drawings: Drawings = cx.data.section().map_err(|e| e.to_string())?;
    if id == "drawing.set_paper" || id == "drawing.set_title" {
        let sheet_id: String = read(cx, "sheet")?;
        let total = drawings.sheets.len();
        let index = drawings
            .sheets
            .iter()
            .position(|s| s.id == sheet_id)
            .ok_or("用紙がありません")?;
        let sheet = &mut drawings.sheets[index];
        if id == "drawing.set_paper" {
            if cx.arg("paper").is_none() && cx.arg("pens").is_none() {
                return Err("用紙かペンの変更する欄を指定してください".into());
            }
            if let Some(patch) = cx.arg("paper") {
                if let Some(colors) = patch.get("colors").and_then(|c| c.as_object())
                    && let Some(key) = colors
                        .keys()
                        .find(|k| !crate::export::LAYERS.contains(&k.as_str()))
                {
                    return Err(format!("画層 {key} はありません。OUTLINE・HIDDEN・CENTER・CUT・HATCH・DIMENSION・TEXT・FRAME から指定してください").into());
                }
                let mut paper = serde_json::to_value(&sheet.paper).map_err(|e| e.to_string())?;
                merge(
                    &mut paper,
                    patch,
                    &["format", "portrait", "binding", "colors"],
                )?;
                sheet.paper = serde_json::from_value(paper).map_err(|e| e.to_string())?;
            }
            if let Some(patch) = cx.arg("pens") {
                let mut pens = serde_json::to_value(&sheet.pens).map_err(|e| e.to_string())?;
                merge(&mut pens, patch, &["thick", "thin", "extra_thick"])?;
                sheet.pens = serde_json::from_value(pens).map_err(|e| e.to_string())?;
            }
            crate::paper::frame(&sheet.paper)?;
        } else {
            let patch: serde_json::Value = read(cx, "fields")?;
            let allowed = crate::paper::field_keys();
            let mut title = crate::paper::title_of(sheet)?;
            let mut fields = serde_json::to_value(&title.fields).map_err(|e| e.to_string())?;
            merge(&mut fields, &patch, &allowed)?;
            title.fields = serde_json::from_value(fields).map_err(|e| e.to_string())?;
            let file = cx.arg_str("font").map(std::path::Path::new);
            let face: u32 = read(cx, "face")?;
            crate::paper::prepare(cx, sheet, &mut title, file, face, index, total)?;
            sheet.title = serde_json::to_value(title).map_err(|e| e.to_string())?;
        }
        let edit = cx.data.set_section(&drawings)?;
        log::info!("図面の用紙の設定 {id}: {sheet_id}");
        return Ok(Outcome::Change {
            label: ACTIONS.iter().find(|a| a.0 == id).unwrap().1.into(),
            edits: vec![edit],
        });
    }
    let date = cx.arg_str("date");
    if let Some(date) = date {
        kmb_format::pdf::validate_date(date)?;
    }
    let selected = cx.arg_str("sheet");
    let sheets = drawings
        .sheets
        .iter()
        .filter(|s| selected.is_none_or(|id| id == s.id))
        .collect::<Vec<_>>();
    if sheets.is_empty() {
        return Err("書き出す用紙がありません".into());
    }
    if id != "drawing.export_pdf" && sheets.len() != 1 {
        return Err("書き出す用紙を一つ指定してください".into());
    }
    let mut pages = Vec::new();
    let mut problems = Vec::new();
    let mut single = Vec::new();
    for sheet in &sheets {
        let (picture, _) = crate::picture::picture_dated(cx, &sheet.id, date)?;
        match id {
            "drawing.export_pdf" => pages.push(crate::export::pdf_page(&picture, sheet)?),
            "drawing.export_svg" => single = crate::export::export_svg(&picture, sheet)?,
            "drawing.export_dxf" => single = crate::export::export_dxf(&picture, sheet)?,
            _ => return Err("知らない図面の書き出しです".into()),
        }
        problems.extend(picture.problems);
    }
    let extension = id.strip_prefix("drawing.export_").unwrap();
    let contents = if extension == "pdf" {
        kmb_format::pdf::write(&pages, date)?
    } else {
        single
    };
    log::info!(
        "図面を書き出す: {extension}・用紙{}枚・問題{}件",
        sheets.len(),
        problems.len()
    );
    Ok(Outcome::File {
        suffix: format!("図面.{extension}"),
        contents,
        info: serde_json::json!({"sheets":sheets.len(),"problems":problems}),
    })
}
impl Extension for ExportCommands {
    fn actions(&self, _: &Context<'_>) -> Vec<Action> {
        ACTIONS
            .iter()
            .map(|(id, label, hint)| {
                Action::new(id, label, "cad")
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
