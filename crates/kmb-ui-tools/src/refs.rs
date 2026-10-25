//! 工具番号で工具ライブラリを指す加工の機能の扱い(番号の振り直しと、PC のライブラリの工具を選ぶ欄)。
//!
//! 加工の機能はデータの `tools`(工具ライブラリの機能の ID)と `tool`・`rest_tool`(工具番号)で工具を
//! 指す。工具ライブラリの番号を変える時は、それを指す加工の番号も同じ取り消し単位で直す。

use crate::{GlobalTools, ToolsEditor, library_of};
use kmb_cam_tool::{TOOLS, TOOLS_V1, Tool, ToolLibrary};
use kmb_core::{DocumentData, FeatureEntry, FeatureId};
use kmb_ui::{APP_EDITS, EditorHost};
use serde_json::{Value as Json, json};

/// 工具番号を持つ欄。
const NUMBER_KEYS: [&str; 2] = ["tool", "rest_tool"];

/// 工具ライブラリ `tools` を `lib` にし、それを指す加工の工具番号を `map`(古い → 新しい)で直す
/// 書き換え([`kmb_ui::APP_EDITS`] の引数)。
pub fn renumber_edits(
    data: &DocumentData,
    tools: FeatureId,
    lib: &ToolLibrary,
    map: &[(u32, u32)],
) -> Json {
    let mut edits =
        vec![json!({"id": tools.0, "data": serde_json::to_value(lib).expect("JSON にできる")})];
    for f in &data.features {
        if f.id == tools || f.data.get("tools").and_then(Json::as_u64) != Some(tools.0) {
            continue;
        }
        let mut d = f.data.clone();
        let mut changed = false;
        for key in NUMBER_KEYS {
            let Some(n) = d.get(key).and_then(Json::as_u64) else {
                continue;
            };
            if let Some((_, new)) = map.iter().find(|(old, _)| *old as u64 == n)
                && *new as u64 != n
            {
                d[key] = json!(new);
                changed = true;
            }
        }
        if changed {
            edits.push(json!({"id": f.id.0, "data": d}));
        }
    }
    json!({"label": "工具の番号を振り直す", "edits": edits})
}

/// 加工の機能 `entry` の工具の欄の下に、PC のライブラリにあって文書のライブラリにない工具を選ぶ欄を
/// 出す。選ぶと、その工具を文書のライブラリへ写し(番号が重なれば空いている番号にする)、加工の工具に
/// する(1 回の取り消し単位)。
pub(crate) fn pc_tool_picker(
    ed: &ToolsEditor,
    ui: &mut egui::Ui,
    host: &mut EditorHost<'_>,
    entry: &FeatureEntry,
) {
    if entry.type_id == TOOLS || entry.type_id == TOOLS_V1 || entry.data.get("tool").is_none() {
        return;
    }
    let Some(tools) = entry
        .data
        .get("tools")
        .and_then(Json::as_u64)
        .map(FeatureId)
    else {
        return;
    };
    let GlobalTools::Loaded(global) = &ed.global else {
        return;
    };
    let Some(lib) = host
        .data
        .feature(tools)
        .filter(|f| f.type_id == TOOLS || f.type_id == TOOLS_V1)
        .and_then(library_of)
    else {
        return;
    };
    let only: Vec<&Tool> = global
        .tools
        .iter()
        .filter(|g| !lib.tools.iter().any(|t| crate::files::same_tool(t, g)))
        .collect();
    if only.is_empty() {
        return;
    }
    let mut picked: Option<Tool> = None;
    ui.horizontal_wrapped(|ui| {
        ui.label("PC のライブラリの工具")
            .on_hover_text("選ぶと、この文書の工具ライブラリへ写して、この加工の工具にする");
        egui::ComboBox::from_id_salt(("pc_tool", entry.id))
            .selected_text("選ぶ…")
            .show_ui(ui, |ui| {
                for t in &only {
                    if ui
                        .selectable_label(false, format!("T{} {}", t.number, t.name))
                        .clicked()
                    {
                        picked = Some((*t).clone());
                    }
                }
            });
    });
    let Some(t) = picked else {
        return;
    };
    let mut lib = lib;
    let name = t.name.clone();
    let Some(&number) = lib.add_tools(vec![t], false).first() else {
        return;
    };
    let mut op = entry.data.clone();
    op["tool"] = json!(number);
    *host.then = Some((
        APP_EDITS.into(),
        json!({
            "label": "PC のライブラリの工具を使う",
            "edits": [
                {"id": tools.0, "data": serde_json::to_value(&lib).expect("JSON にできる")},
                {"id": entry.id.0, "data": op},
            ],
            "message": format!(
                "PC のライブラリの工具「{name}」を、文書の工具ライブラリへ T{number} として写しました"
            ),
        }),
    ));
}
