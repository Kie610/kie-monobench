//! DXF の操作(画面・CLI・MCP に出す): スケッチへの挿入と、スケッチの DXF での保存。

use kmb_core::{Action, Context, Extension, Input, Outcome, Place};

/// DXF を挿入(選んでいるスケッチへ。なければ XY 平面の新しいスケッチ)
pub const INSERT_DXF: &str = "cad.sketch.insert_dxf";
/// 選んでいるスケッチを DXF で保存
pub const EXPORT_DXF: &str = "cad.sketch.export_dxf";

/// DXF の操作。
pub struct DxfCommands;

/// 選んでいるスケッチを DXF にする(補助線は書かない)。
fn export_dxf(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, sketch) = kmb_sketch::selected_sketch(cx).ok_or("スケッチを選んでから使う")?;
    let sol = cx
        .output(id, "sketch")
        .and_then(kmb_sketch::solution_of)
        .ok_or("スケッチを計算できていないので書き出せません")?;
    let text = crate::to_dxf(&sketch, &sol, &crate::DxfOptions::default());
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
    /// 画面の並びは前と同じ(挿入はスケッチの平面の再定義より前、保存は文字の分解の後)。
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let sketch = kmb_sketch::selected_sketch(cx).is_some();
        vec![
            Action::new(INSERT_DXF, "DXF を挿入…", "cad")
                .before(kmb_sketch::REDEFINE_PLANE)
                .role(kmb_interchange::role::action::INSERT_DXF)
                .menu("挿入")
                .icon("dxf")
                .aliases("でぃーえっくすえふ dxf そうにゅう いんぽーと import 2d")
                .hint("DXF の線・円弧・円・点を、選んでいるスケッチへ足す。スケッチを選んでいなければ XY 平面に新しく作る")
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

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        Some(match id {
            INSERT_DXF => kmb_sketch::insert_file(cx, "DXF", crate::from_dxf),
            EXPORT_DXF => export_dxf(cx),
            _ => return None,
        })
    }
}
