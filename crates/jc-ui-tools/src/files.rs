//! 工具ライブラリのファイルの読み込みと書き出し(形の変換は `jc_cam_tool::exchange`)。

use jc_cam_tool::exchange::{read_library, write_library};
use jc_cam_tool::{Tool, ToolLibrary};
use serde_json::Value as Json;
use std::path::Path;

/// 番号と知らない項目のほかは同じ工具か(同じファイルを 2 回読んでも、書き出したファイルを読み直しても
/// 増やさないため。形を変換した時の丸めの差は無視する)。
pub(crate) fn same_tool(a: &Tool, b: &Tool) -> bool {
    let plain = |t: &Tool| {
        serde_json::to_value(Tool {
            number: 0,
            extra: Default::default(),
            ..t.clone()
        })
        .unwrap_or_default()
    };
    close(&plain(a), &plain(b))
}

/// JSON が同じか(数は 1e-6 の差まで同じとみなす)。
fn close(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Number(x), Json::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= 1e-6 * x.abs().max(1.0),
            _ => x == y,
        },
        (Json::Array(x), Json::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| close(x, y))
        }
        (Json::Object(x), Json::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| close(v, w)))
        }
        _ => a == b,
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    )
}

/// `path` の工具ライブラリ(この CAM の `tools.json`)を読み、`lib` に足す。
/// 番号が重なる工具は空いている番号にする。知らせの文を返す。
pub fn import_library(lib: &mut ToolLibrary, path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{} を読めません: {e}", path.display()))?;
    let mut im = read_library(&bytes).map_err(|e| format!("{}: {e}", file_name(path)))?;
    let total = im.tools.len();
    let fresh: Vec<Tool> = std::mem::take(&mut im.tools)
        .into_iter()
        .filter(|t| !lib.tools.iter().any(|o| same_tool(o, t)))
        .collect();
    let same = total - fresh.len();
    let added = lib.add_tools(fresh, false);
    let mut msg = match (added.first(), added.last()) {
        (Some(a), Some(b)) => format!(
            "{} から工具 {} 本を読み込みました(T{a}〜T{b})",
            file_name(path),
            added.len()
        ),
        _ => format!("{} から足す工具はありませんでした", file_name(path)),
    };
    if same > 0 {
        msg += &format!("。同じ工具 {same} 本はもうあるので足していません");
    }
    if im.skipped_count() > 0 {
        msg += &format!(
            "。読めない工具 {} 本を飛ばしました({})",
            im.skipped_count(),
            im.skipped_text()
        );
    }
    Ok(msg)
}

/// `lib` を `path` へ書く。拡張子が `.csv` なら CSV、`.tools` なら他の CAM の工具表の zip、ほかはその JSON。
/// 知らせの文を返す。
pub fn export_library(lib: &ToolLibrary, path: &Path) -> Result<String, String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let bytes = write_library(lib, &ext);
    jc_session::write_output(path, &bytes)?;
    let form = match ext.as_str() {
        "csv" => "CSV",
        "tools" => "ほかの CAM の .tools",
        _ => "ほかの CAM の JSON",
    };
    Ok(format!(
        "工具 {} 本を {} に書き出しました({form})",
        lib.tools.len(),
        file_name(path)
    ))
}
