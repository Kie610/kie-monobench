//! 図面の注記を、共通の引数の宣言と一回の区画の変更で作る。

use crate::commands::{optional, read};
use crate::*;
use kmb_core::{Action, Arg, Context, Extension, Outcome, Payload, Problem};
use std::sync::Mutex;

#[derive(Default)]
pub(super) struct AnnotationCommands {
    fonts: Mutex<kmb_text::fonts::FontIndex>,
}
const ACTIONS: &[(&str, &str, &str)] = &[
    (
        "drawing.dimension",
        "寸法",
        "ビューの辺を参照し、形の値を表示する寸法を置く",
    ),
    (
        "drawing.dimension_set",
        "寸法をまとめて置く",
        "選んだ点の列から並列・直列・座標の寸法を一回だけ置く",
    ),
    (
        "drawing.import_dimensions",
        "モデルの寸法を取り込む",
        "選んだスケッチの寸法を、ビューの見える辺を参照する寸法として一回だけ取り込む",
    ),
    ("drawing.note", "注記", "用紙に文字か引出線付きの文字を置く"),
    (
        "drawing.center_mark",
        "中心の印",
        "円や円弧の中心に十字の印を付ける",
    ),
    (
        "drawing.center_line",
        "中心線",
        "平行な二本の辺の中間か、円筒の両端の円から中心線を引く",
    ),
    (
        "drawing.balloon",
        "番号札",
        "部品表の行に対応する番号札を利用者が決めた位置に置く",
    ),
    (
        "drawing.renumber_balloons",
        "番号札を振り直す",
        "番号札の番号を現在の部品表の行の順に合わせる",
    ),
    (
        "drawing.align_balloons",
        "番号札をそろえる",
        "選んだ番号札を最初の札と同じ縦か横の位置にそろえる",
    ),
    (
        "drawing.parts_table",
        "部品表",
        "選んだ列の部品表を線と文字にして用紙に置く",
    ),
    (
        "drawing.set_style",
        "寸法の様式",
        "文字・矢印・補助線・桁・単位・公差の様式を用紙ごとに決める",
    ),
];
fn args(id: &str) -> Vec<Arg> {
    let mut a = vec![Arg::text("sheet", "用紙のid").required()];
    match id {
        "drawing.set_style" => {
            a.push(Arg::value("style","変更する様式の欄。height・arrow・arrow_length・arrow_angle・extension_gap・extension_over・spacing・digits・trim_zeros・unit・tolerance").required());
            return a;
        }
        "drawing.renumber_balloons" => return a,
        "drawing.import_dimensions" => {
            a.extend([
                Arg::text("id", "取り込む寸法のidの頭").required(),
                Arg::text("view", "ビューのid").required(),
                Arg::value("features", "寸法を取り込むスケッチの機能の番号の列").required(),
            ]);
            return a;
        }
        "drawing.align_balloons" => {
            a.extend([
                Arg::value("ids", "そろえる番号札のidの列。最初の札の位置を基準にする").required(),
                Arg::text("axis", "xは同じ縦の位置、yは同じ横の位置").default("x"),
            ]);
            return a;
        }
        _ => {}
    }
    a.push(Arg::text("id", "注記のid。まとめる寸法はidの後ろに連番を足す").required());
    match id {
        "drawing.dimension" | "drawing.dimension_set" => {
            a.extend([Arg::value("dimension","horizontal・vertical・parallel・angle・radius・diameter・ordinate_x・ordinate_y").required(),Arg::value("refs","参照の列 {view,key:{body,faces,index},at:a|b|midpoint|center|line}").required(),Arg::value("at","寸法線の用紙のmmの位置[x,y]").required()]);
            if id == "drawing.dimension_set" {
                a.push(
                    Arg::text("mode", "baseline(並列)・chain(直列)・ordinate(座標)")
                        .default("baseline"),
                );
            }
        }
        "drawing.note" => a.extend([
            Arg::text("text", "注記の文字").required(),
            Arg::value("at", "文字の用紙のmmの位置[x,y]").required(),
            Arg::value(
                "leader",
                "引出線 {tip:[x,y],reference:参照(任意),arrow:triangle|dot|slash}",
            ),
        ]),
        "drawing.center_mark" => {
            a.push(Arg::value("reference", "円か円弧の参照 {view,key,at}").required())
        }
        "drawing.center_line" => a.push(Arg::value("refs", "二本の平行な辺の参照の列").required()),
        "drawing.balloon" => a.extend([
            Arg::text("view", "ビューのid").required(),
            Arg::value("part", "部品{kind:component,id}かボディ{kind:body,root}").required(),
            Arg::value("tip", "指す点の用紙のmmの位置[x,y]").required(),
            Arg::value("at", "札の中心の用紙のmmの位置[x,y]").required(),
        ]),
        "drawing.parts_table" => a.extend([
            Arg::value("at", "表の左上の用紙のmmの位置。省くと右下"),
            Arg::value(
                "columns",
                "列のidの列。既定はitem/name/count/material/length/width/thickness/mass",
            ),
        ]),
        _ => {}
    }
    a
}
fn canonical(
    reference: &mut DimRef,
    views: &std::collections::BTreeMap<String, crate::dimension::ViewGeometry>,
) {
    if let Some(view) = views.get(&reference.view) {
        reference.key = view.canonical_key(&reference.key);
    }
}
fn upsert(sheet: &mut Sheet, note: Annotation) -> Result<(), Problem> {
    let value = serde_json::to_value(&note).map_err(|e| e.to_string())?;
    if let Some(old) = sheet
        .notes
        .iter_mut()
        .find(|n| n.get("id").and_then(|v| v.as_str()) == Some(&note.id))
    {
        if old.get("kind") != value.get("kind") {
            return Err("このidは別の種類の注記に使っています。新しいidを指定してください".into());
        }
        let target = old.as_object_mut().ok_or("注記を読めません")?;
        target.extend(
            value
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
    } else {
        sheet.notes.push(value);
    }
    Ok(())
}
fn dimension_set(
    prefix: &str,
    kind: DimKind,
    refs: &[DimRef],
    at: [f64; 2],
    mode: &str,
    offset: [f64; 2],
) -> Result<Vec<Annotation>, String> {
    if refs.len() < 2 {
        return Err("まとめる寸法には点の参照を2つ以上指定してください".into());
    }
    if !matches!(
        kind,
        DimKind::Horizontal | DimKind::Vertical | DimKind::Parallel
    ) || !matches!(mode, "baseline" | "chain" | "ordinate")
    {
        return Err("並列・直列・座標と、水平・垂直・平行の寸法を指定してください".into());
    }
    if mode == "ordinate" && kind == DimKind::Parallel {
        return Err("座標の寸法は水平か垂直で指定してください".into());
    }
    Ok((1..refs.len())
        .map(|i| {
            let mut at = at;
            if mode != "chain" {
                at = crate::dimension::add(at, crate::dimension::scale(offset, (i - 1) as f64));
            }
            let dimension = if mode == "ordinate" {
                if kind == DimKind::Horizontal {
                    DimKind::OrdinateX
                } else {
                    DimKind::OrdinateY
                }
            } else {
                kind
            };
            Annotation {
                id: format!("{prefix}-{i}"),
                kind: AnnotationKind::Dimension {
                    dimension,
                    refs: vec![
                        refs[if mode == "chain" { i - 1 } else { 0 }].clone(),
                        refs[i].clone(),
                    ],
                    at,
                },
                outline: None,
                rest: Rest::new(),
            }
        })
        .collect())
}
impl AnnotationCommands {
    fn execute(&self, id: &str, cx: &Context<'_>) -> Result<Outcome, Problem> {
        let mut drawings: Drawings = cx.data.section().map_err(|e| e.to_string())?;
        let sheet_id: String = read(cx, "sheet")?;
        let sheet = drawings
            .sheets
            .iter_mut()
            .find(|s| s.id == sheet_id)
            .ok_or("用紙がありません")?;
        match id {
            "drawing.set_style" => {
                let style: serde_json::Value = read(cx, "style")?;
                let input = style
                    .as_object()
                    .ok_or("様式は欄の名前と値で指定してください")?;
                let keys = [
                    "height",
                    "arrow",
                    "arrow_length",
                    "arrow_angle",
                    "extension_gap",
                    "extension_over",
                    "spacing",
                    "digits",
                    "trim_zeros",
                    "unit",
                    "tolerance",
                ];
                if let Some(key) = input.keys().find(|key| !keys.contains(&key.as_str())) {
                    return Err(format!("様式の欄 {key} はありません").into());
                }
                if sheet.dim_style.is_null() {
                    sheet.dim_style = serde_json::json!({});
                }
                let target = sheet
                    .dim_style
                    .as_object_mut()
                    .ok_or("保存した寸法の様式を読めません")?;
                for (k, v) in input {
                    if k == "tolerance"
                        && let (Some(a), Some(b)) = (
                            target.get_mut(k).and_then(|v| v.as_object_mut()),
                            v.as_object(),
                        )
                    {
                        a.extend(b.iter().map(|(k, v)| (k.clone(), v.clone())));
                        continue;
                    }
                    target.insert(k.clone(), v.clone());
                }
                style_of(&sheet.dim_style)?;
            }
            "drawing.import_dimensions" => {
                let prefix: String = read(cx, "id")?;
                let view: String = read(cx, "view")?;
                let features: Vec<kmb_core::FeatureId> = read(cx, "features")?;
                let views = crate::picture::view_references(cx, sheet);
                let space = views.get(&view).ok_or("ビューがありません")?;
                let mut count = 0;
                let mut skipped = Vec::new();
                for id in features {
                    let result = (|| -> Result<_, String> {
                        let entry = cx.data.feature(id).ok_or("スケッチがありません")?;
                        if cx.registry.category(&entry.type_id) != Some(kmb_core::category::SKETCH)
                        {
                            return Err("平面のスケッチを選んでください".into());
                        }
                        let sketch: kmb_sketch::Sketch = serde_json::from_value(entry.data.clone())
                            .map_err(|e| e.to_string())?;
                        let points = kmb_core::Points::from_value(
                            cx.output(id, "points")
                                .ok_or("スケッチを再計算してください")?,
                        )
                        .ok_or("スケッチの平面を読めません")?;
                        let sol = kmb_sketch::solution_of(
                            cx.output(id, "sketch")
                                .ok_or("スケッチを再計算してください")?,
                        )
                        .ok_or("スケッチの解を読めません")?;
                        Ok(crate::import_dimensions::from_sketch(
                            &prefix,
                            id,
                            &sketch,
                            &sol,
                            points.plane,
                            &view,
                            space,
                        ))
                    })();
                    match result {
                        Err(e) => skipped.push(format!("{}: {e}", id.0)),
                        Ok(result) => {
                            skipped.extend(result.skipped);
                            for note in result.notes {
                                if sheet
                                    .notes
                                    .iter()
                                    .any(|n| n.get("id").and_then(|v| v.as_str()) == Some(&note.id))
                                {
                                    skipped.push(format!("{}は既にあります", note.id));
                                    continue;
                                }
                                upsert(sheet, note)?;
                                count += 1;
                            }
                        }
                    }
                }
                for why in &skipped {
                    log::warn!("モデルの寸法の取り込みを見送り: {why}");
                }
                if count == 0 {
                    return Ok(Outcome::Text {
                        title: "モデルの寸法を取り込む".into(),
                        body: if skipped.is_empty() {
                            "取り込める寸法がありません".into()
                        } else {
                            skipped.join("\n")
                        },
                    });
                }
                return finish(
                    drawings,
                    cx,
                    &format!(
                        "モデルの寸法を取り込む(追加{count}・見送り{})",
                        skipped.len()
                    ),
                );
            }
            "drawing.renumber_balloons" => {
                let (_, sources) = kmb_asm::inspect::bom_rows_with_sources(cx);
                for note in &mut sheet.notes {
                    if note.get("kind").and_then(|v| v.as_str()) != Some("balloon") {
                        continue;
                    }
                    let part: kmb_asm::inspect::BomSource = serde_json::from_value(
                        note.get("part")
                            .ok_or("番号札に部品の参照がありません")?
                            .clone(),
                    )
                    .map_err(|e| e.to_string())?;
                    let number = sources
                        .get(&part)
                        .ok_or("番号札の部品がありません。部品を選び直してください")?
                        + 1;
                    note["number"] = serde_json::json!(number);
                }
            }
            "drawing.align_balloons" => {
                let ids: Vec<String> = read(cx, "ids")?;
                let axis: String = optional(cx, "axis")?;
                if ids.len() < 2 || !matches!(axis.as_str(), "x" | "y") {
                    return Err("番号札を2つ以上と、そろえる軸xかyを指定してください".into());
                }
                let mut at = None;
                for id in ids {
                    let note = sheet
                        .notes
                        .iter_mut()
                        .find(|n| n.get("id").and_then(|v| v.as_str()) == Some(&id))
                        .ok_or("指定した番号札がありません")?;
                    if note.get("kind").and_then(|v| v.as_str()) != Some("balloon") {
                        return Err("そろえる対象は番号札にしてください".into());
                    }
                    let mut p: [f64; 2] =
                        serde_json::from_value(note["at"].clone()).map_err(|e| e.to_string())?;
                    if let Some(a) = at {
                        p[if axis == "x" { 0 } else { 1 }] = a;
                    } else {
                        at = Some(p[if axis == "x" { 0 } else { 1 }]);
                    }
                    note["at"] = serde_json::json!(p);
                }
            }
            _ => {
                let target: String = read(cx, "id")?;
                if target.trim().is_empty() {
                    return Err("注記のidを指定してください".into());
                }
                let views = crate::picture::view_references(cx, sheet);
                let style = style_of(&sheet.dim_style)?;
                let mut note = Annotation {
                    id: target.clone(),
                    kind: AnnotationKind::Note {
                        text: String::new(),
                        at: [0.0; 2],
                        leader: None,
                    },
                    outline: None,
                    rest: Rest::new(),
                };
                match id {
                    "drawing.dimension" | "drawing.dimension_set" => {
                        let kind: DimKind = read(cx, "dimension")?;
                        let mut refs: Vec<DimRef> = read(cx, "refs")?;
                        let at = read(cx, "at")?;
                        for reference in &mut refs {
                            canonical(reference, &views);
                        }
                        if id == "drawing.dimension_set" {
                            let mode: String = optional(cx, "mode")?;
                            let offset = if kind == DimKind::Parallel {
                                if refs.len() < 2 {
                                    return Err(
                                        "まとめる寸法には点を2つ以上指定してください".into()
                                    );
                                }
                                let a = crate::dimension::resolve(&refs[0], &views)?.point;
                                let b = crate::dimension::resolve(&refs[1], &views)?.point;
                                let d = crate::dimension::sub(b, a);
                                let length = crate::dimension::norm(d);
                                if length < 1e-12 {
                                    return Err(
                                        "平行の寸法の向きを決める二点を離してください".into()
                                    );
                                }
                                [
                                    -d[1] * style.spacing / length,
                                    d[0] * style.spacing / length,
                                ]
                            } else if kind == DimKind::Vertical {
                                [style.spacing, 0.0]
                            } else {
                                [0.0, style.spacing]
                            };
                            let made = dimension_set(&target, kind, &refs, at, &mode, offset)?;
                            for note in &made {
                                if sheet
                                    .notes
                                    .iter()
                                    .any(|n| n.get("id").and_then(|v| v.as_str()) == Some(&note.id))
                                {
                                    return Err(
                                        "このidの寸法は既にあります。別のidで置いてください".into(),
                                    );
                                }
                            }
                            for note in &made {
                                crate::picture::validate_annotation(cx, sheet, note, &views)?;
                            }
                            for note in made {
                                upsert(sheet, note)?;
                            }
                            return finish(drawings, cx, "寸法をまとめて置く");
                        }
                        note.kind = AnnotationKind::Dimension {
                            dimension: kind,
                            refs,
                            at,
                        };
                    }
                    "drawing.note" => {
                        let text: String = read(cx, "text")?;
                        if text.trim().is_empty() {
                            return Err("注記の文字を入れてください".into());
                        }
                        let mut leader: Option<Leader> = optional(cx, "leader")?;
                        if let Some(leader) = leader.as_mut()
                            && let Some(reference) = leader.reference.as_mut()
                        {
                            canonical(reference, &views);
                        }
                        note.outline = crate::lettering::embed(
                            &mut self.fonts.lock().unwrap_or_else(|e| e.into_inner()),
                            &text,
                        )?;
                        note.kind = AnnotationKind::Note {
                            text,
                            at: read(cx, "at")?,
                            leader,
                        };
                    }
                    "drawing.center_mark" => {
                        let mut reference = read(cx, "reference")?;
                        canonical(&mut reference, &views);
                        note.kind = AnnotationKind::CenterMark { reference };
                    }
                    "drawing.center_line" => {
                        let mut refs: [DimRef; 2] = read(cx, "refs")?;
                        for r in &mut refs {
                            canonical(r, &views);
                        }
                        note.kind = AnnotationKind::CenterLine { refs };
                    }
                    "drawing.balloon" => {
                        let view: String = read(cx, "view")?;
                        let mut part: kmb_asm::inspect::BomSource = read(cx, "part")?;
                        if let kmb_asm::inspect::BomSource::Body { root } = &mut part {
                            *root = kmb_core::body_root(cx.data, cx.registry, *root);
                        }
                        let p = views.get(&view).ok_or("ビューがありません")?;
                        let roots = match &part {
                            kmb_asm::inspect::BomSource::Component { id } => cx
                                .data
                                .component(*id)
                                .ok_or("部品がありません")?
                                .bodies
                                .clone(),
                            kmb_asm::inspect::BomSource::Body { root } => vec![*root],
                        };
                        if !roots.iter().any(|r| p.bodies.contains(r)) {
                            return Err("指定した部品はこのビューにありません".into());
                        }
                        let (_, sources) = kmb_asm::inspect::bom_rows_with_sources(cx);
                        let number = sources
                            .get(&part)
                            .ok_or("部品表に指定した部品がありません")?
                            + 1;
                        let tip: [f64; 2] = read(cx, "tip")?;
                        let tip = std::array::from_fn(|i| (tip[i] - p.at[i]) / p.scale);
                        note.kind = AnnotationKind::Balloon {
                            view,
                            part,
                            tip,
                            at: read(cx, "at")?,
                            number,
                        };
                    }
                    "drawing.parts_table" => {
                        let columns: Vec<String> = optional(cx, "columns")?;
                        let rows = kmb_asm::inspect::bom_rows(cx);
                        let table = kmb_asm::inspect::bom_table(&rows);
                        for key in &columns {
                            if !table.columns.iter().any(|c| c.id == *key) {
                                return Err(format!("部品表に列 {key} はありません").into());
                            }
                        }
                        let selected = table
                            .columns
                            .iter()
                            .enumerate()
                            .filter(|(_, c)| {
                                if columns.is_empty() {
                                    [
                                        "item",
                                        "name",
                                        "count",
                                        "material",
                                        "length",
                                        "width",
                                        "thickness",
                                        "mass",
                                    ]
                                    .contains(&c.id.as_str())
                                } else {
                                    columns.contains(&c.id)
                                }
                            })
                            .map(|(i, _)| i)
                            .collect::<Vec<_>>();
                        let text = selected
                            .iter()
                            .map(|i| table.columns[*i].label.as_str())
                            .chain(
                                table
                                    .rows
                                    .iter()
                                    .flat_map(|row| selected.iter().map(|i| row[*i].as_str())),
                            )
                            .collect::<Vec<_>>()
                            .join("\n");
                        note.outline = crate::lettering::embed(
                            &mut self.fonts.lock().unwrap_or_else(|e| e.into_inner()),
                            &text,
                        )?;
                        note.kind = AnnotationKind::PartsTable {
                            at: optional(cx, "at")?,
                            columns,
                        };
                    }
                    _ => unreachable!(),
                }
                crate::picture::validate_annotation(cx, sheet, &note, &views)?;
                upsert(sheet, note)?;
            }
        }
        finish(drawings, cx, ACTIONS.iter().find(|a| a.0 == id).unwrap().1)
    }
}
fn finish(drawings: Drawings, cx: &Context<'_>, label: &str) -> Result<Outcome, Problem> {
    let edit = cx.data.set_section(&drawings)?;
    log::info!("図面の注記の操作 {label}: 用紙{}枚", drawings.sheets.len());
    Ok(Outcome::Change {
        label: label.into(),
        edits: vec![edit],
    })
}
impl Extension for AnnotationCommands {
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
        ACTIONS
            .iter()
            .any(|a| a.0 == id)
            .then(|| self.execute(id, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dimension_sets_space_new_baselines_without_rearranging_saved_notes() {
        let reference = DimRef {
            view: "v".into(),
            key: kmb_interchange::drawing::EdgeKey {
                body: kmb_core::FeatureId(1),
                faces: ["a".into(), "b".into()],
                index: 0,
            },
            at: RefAt::A,
        };
        let notes = dimension_set(
            "d",
            DimKind::Horizontal,
            &[reference.clone(), reference.clone(), reference],
            [10.0, 20.0],
            "baseline",
            [0.0, 8.0],
        )
        .unwrap();
        assert_eq!(notes.len(), 2);
        assert!(matches!(
            notes[0].kind,
            AnnotationKind::Dimension {
                at: [10.0, 20.0],
                ..
            }
        ));
        assert!(matches!(
            notes[1].kind,
            AnnotationKind::Dimension {
                at: [10.0, 28.0],
                ..
            }
        ));
        let mut sheet = Sheet::new("s", "図面");
        sheet
            .notes
            .push(serde_json::json!({"id":"old","kind":"future","at":[1,2],"future":7}));
        let before = sheet.notes[0].clone();
        for n in notes {
            upsert(&mut sheet, n).unwrap();
        }
        assert_eq!(sheet.notes[0], before);
    }
}
