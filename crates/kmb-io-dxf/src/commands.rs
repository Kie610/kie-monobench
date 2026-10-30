//! DXF の操作(画面・CLI・MCP に出す): スケッチへの挿入と、スケッチの DXF での保存。

use kmb_core::{Action, Arg, Context, Extension, Input, Outcome, Place};

/// DXF を挿入(選んでいるスケッチへ。なければ XY 平面の新しいスケッチ)
pub const INSERT_DXF: &str = "cad.sketch.insert_dxf";
/// 選んでいるスケッチを DXF で保存
pub const EXPORT_DXF: &str = "cad.sketch.export_dxf";
/// ファイルの画層を読む。
pub const LIST_DXF_LAYERS: &str = "cad.sketch.dxf_layers";

/// DXF の操作。
pub struct DxfCommands;

fn insert_dxf(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let path = cx.file.ok_or("DXF のファイルを選んでから使う")?;
    let bytes = std::fs::read(path).map_err(|e| {
        kmb_core::Problem::new(
            kmb_core::ProblemKind::File,
            format!("{} を読めません: {e}", path.display()),
        )
    })?;
    let selected = cx
        .arg("layers")
        .map(|value| {
            value
                .as_array()
                .ok_or("画層は名前の並びで指定してください")?
                .iter()
                .map(|name| {
                    name.as_str()
                        .map(String::from)
                        .ok_or("画層の名前は文字列で指定してください")
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let read = crate::from_dxf_bytes_with_layers(&bytes, selected.as_deref())?;
    let excluded = read.excluded;
    let mut read = read.imported;
    let cleanup = cx.arg("cleanup").and_then(|v| v.as_bool()).unwrap_or(true);
    let tolerance = cx.arg_f64("cleanup_tolerance")?.unwrap_or(0.01);
    crate::clean::validate_tolerance(tolerance)?;
    let mut counts = None;
    if cleanup {
        let clean = crate::clean(&read.shapes, tolerance)?;
        counts = Some((clean.removed, clean.joined));
        read.bounds = kmb_sketch::shapes_bounds(&clean.shapes);
        read.shapes = clean.shapes;
    }
    let mut result = kmb_sketch::insert_imported(cx, "DXF", read)?;
    if let (Some((removed, joined)), Outcome::Change { label, .. }) = (counts, &mut result) {
        *label = format!("{label}(重複を {removed} 本除去、端点を {joined} 組接続)");
    }
    if excluded > 0
        && let Outcome::Change { label, .. } = &mut result
    {
        *label = format!("{label}(選ばない画層の形 {excluded} 個を除外)");
    }
    Ok(result)
}

fn list_layers(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let argument = cx.arg_str("file").map(std::path::PathBuf::from);
    if let (Some(file), Some(arg)) = (cx.file, argument.as_deref())
        && file != arg
    {
        return Err("DXF のファイルは一つだけ指定してください".into());
    }
    let path = cx
        .file
        .or(argument.as_deref())
        .ok_or("DXF のファイルを指定してください")?;
    let path = cx.open_path(path)?;
    let bytes = std::fs::read(&path).map_err(|e| {
        kmb_core::Problem::new(
            kmb_core::ProblemKind::File,
            format!("{} を読めません: {e}", path.display()),
        )
    })?;
    let read = crate::from_dxf_bytes_with_layers(&bytes, None)?;
    let layers: Vec<_> = read
        .layers
        .into_iter()
        .map(|layer| serde_json::json!({"name":layer.name,"shapes":layer.shapes}))
        .collect();
    Ok(Outcome::Text {
        title: "DXF の画層".into(),
        body: serde_json::to_string_pretty(&layers).expect("画層の一覧"),
    })
}

/// 選んでいるスケッチを DXF にする(補助線は書かない)。
fn export_dxf(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let (id, sketch) = kmb_sketch::selected_sketch(cx).ok_or("スケッチを選んでから使う")?;
    let sol = cx
        .output(id, "sketch")
        .and_then(kmb_sketch::solution_of)
        .ok_or("スケッチを計算できていないので書き出せません")?;
    let text = crate::to_dxf(&sketch, &sol, &crate::DxfOptions::default())
        .map_err(|e| kmb_core::Problem::new(kmb_core::ProblemKind::Compute, e))?;
    let name = cx
        .data
        .feature(id)
        .map_or("スケッチ".into(), |f| f.name.clone());
    Ok(Outcome::File {
        suffix: format!("{name}.dxf"),
        contents: text.into_bytes(),
        info: serde_json::json!({"entities": sketch.entities.len()}),
    })
}

impl Extension for DxfCommands {
    /// 挿入はスケッチの平面の再定義より前、保存は文字の分解の後に並ぶ。
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let sketch = kmb_sketch::selected_sketch(cx).is_some();
        vec![
            Action::new(LIST_DXF_LAYERS, "DXF の画層を一覧", "cad")
                .args([Arg::file("file", "DXF のファイルのパス(省くとファイルの指定で渡す)")])
                .hint("DXF の画層の名前と形の数を読む。設計は変えない")
                .input(Input::OpenFile { title: "DXF の画層を一覧".into(), filter: "DXF".into(), extensions: vec!["dxf".into(),"DXF".into()] }),
            Action::new(INSERT_DXF, "DXF を挿入…", "cad")
                .before(kmb_sketch::REDEFINE_PLANE)
                .role(kmb_interchange::role::action::INSERT_DXF)
                .menu("挿入")
                .icon("dxf")
                .aliases("でぃーえっくすえふ dxf そうにゅう いんぽーと import 2d")
                .hint("DXF の線・円弧・円・点を、選んでいるスケッチへ足す。スケッチを選んでいなければ XY 平面に新しく作る")
                .args(vec![Arg::list("layers","挿入する画層の名前の並び(省くと全部)"), Arg::flag("cleanup","重複した線をまとめ、近い端点をつなぐ").default(true), Arg::length("cleanup_tolerance","端点をつなぐ許容差(mm)").default(0.01)])
                                .input(Input::OpenFile {
                    title: "DXF を挿入".into(),
                    filter: "DXF".into(),
                    extensions: ["dxf", "DXF"].map(String::from).to_vec(),
                }),
            Action::new(EXPORT_DXF, "DXF として保存…", "cad")
                .role(kmb_interchange::role::action::EXPORT_SKETCH_DXF)
                .menu("書き出し")
                .icon("dxf")
                .aliases("でぃーえっくすえふ dxf ほぞん かきだし export れーざー")
                .hint("選んでいるスケッチの線(補助線を除く)を DXF に書き出す")
                .at(if sketch {
                    &[Place::FeatureMenu]
                } else {
                    &[]
                })
                .input(Input::SaveFile {
                    title: "DXF として保存".into(),
                    filter: "DXF".into(),
                    extensions: vec!["dxf".into()],
                })
                .disabled_if((!sketch).then_some("スケッチを選んでから使う")),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        Some(match id {
            INSERT_DXF => insert_dxf(cx),
            EXPORT_DXF => export_dxf(cx),
            LIST_DXF_LAYERS => list_layers(cx),
            _ => return None,
        })
    }
}
