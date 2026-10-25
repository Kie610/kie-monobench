//! SVG の操作(画面・CLI・MCP に出す): スケッチへの挿入と、スケッチの SVG での保存。

use kmb_core::{Action, Context, Extension, Input, Outcome, Place};

/// SVG を挿入(選んでいるスケッチへ。なければ XY 平面の新しいスケッチ)
pub const INSERT_SVG: &str = "cad.sketch.insert_svg";
/// 選んでいるスケッチを SVG で保存(「DXF として保存」と同じく、ブラウザのスケッチの右クリック)。
/// 引数 `construction` が真なら補助線も青の破線で書く
pub const EXPORT_SVG: &str = "cad.sketch.export_svg";

/// 選んでいるスケッチを SVG にする(既定は補助線を書かない。「DXF として保存」と同じ)。
fn export_svg(cx: &Context<'_>) -> Result<Outcome, String> {
    let (id, sketch) = kmb_sketch::selected_sketch(cx).ok_or("スケッチを選んでから使う")?;
    let sol = cx
        .output(id, "sketch")
        .and_then(kmb_sketch::solution_of)
        .ok_or("スケッチを計算できていないので書き出せません")?;
    let name = cx
        .data
        .feature(id)
        .map_or("スケッチ".into(), |f| f.name.clone());
    let opts = crate::SvgOptions {
        construction: cx.arg("construction").and_then(|v| v.as_bool()) == Some(true),
        title: name.clone(),
    };
    let text = crate::to_svg(&sketch, &sol, &opts).ok_or("書き出す線がありません")?;
    log::info!(
        "{name} を SVG に書き出す(要素 {} 個)",
        sketch.entities.len()
    );
    Ok(Outcome::File {
        suffix: format!("{name}.svg"),
        contents: text.into_bytes(),
        info: serde_json::json!({"entities": sketch.entities.len()}),
    })
}

/// SVG の操作。
pub struct SvgCommands;

impl Extension for SvgCommands {
    /// 画面の並びは前と同じ(DXF の挿入の後、スケッチの平面の再定義より前)。
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let sketch = kmb_sketch::selected_sketch(cx).is_some();
        vec![
            Action::new(INSERT_SVG, "SVG を挿入…", "cad")
                .before(kmb_sketch::REDEFINE_PLANE)
                .role(kmb_interchange::role::action::INSERT_SVG)
                .menu("挿入")
                .icon("svg")
                .aliases("えすぶいじー svg そうにゅう いんぽーと import がぞう")
                .hint("SVG の線と曲線を、選んでいるスケッチへ足す(曲線は線分と円弧に直す)。スケッチを選んでいなければ XY 平面に新しく作る")
                // 設計のツールバーの挿入に出す(STEP の読み込みの次)
                .at(&[Place::Toolbar])
                .input(Input::OpenFile {
                    title: "SVG を挿入".into(),
                    filter: "SVG".into(),
                    extensions: ["svg", "SVG"].map(String::from).to_vec(),
                }),
            Action::new(EXPORT_SVG, "SVG として保存…", "cad")
                .arg(kmb_core::Arg::flag("construction", "補助線も書く"))
                .menu("書き出し")
                .icon("svg")
                .aliases("えすぶいじー svg ほぞん かきだし export れーざー lightburn らいとばーん")
                .hint("選んでいるスケッチの線(補助線を除く)を、1 mm = 1 単位の SVG に書き出す(LightBurn などのレーザー加工機のソフトで同じ大きさで開ける。つながる線は閉じた輪郭にまとめる)")
                .at(if sketch {
                    &[Place::FeatureMenu]
                } else {
                    &[]
                })
                .input(Input::SaveFile {
                    title: "SVG として保存".into(),
                    filter: "SVG".into(),
                    extensions: vec!["svg".into()],
                })
                .disabled_if((!sketch).then_some("スケッチを選んでから使う")),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>> {
        Some(match id {
            INSERT_SVG => kmb_sketch::insert_file(cx, "SVG", crate::from_svg),
            EXPORT_SVG => export_svg(cx),
            _ => return None,
        })
    }
}
