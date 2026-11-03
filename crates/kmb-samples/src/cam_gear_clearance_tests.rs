//! 確かめること: 歯車の歯溝へ入らない工具で 2D 輪郭を削る時、取り残しを利用者へ知らせる。
//! CAD と加工を組み立てる試験であり、加工の crate は歯車の crate に依存しない。

use crate::sample::DocBuilder;
use kmb_cam_setup::{SETUP, Setup, toolpath_of};
use kmb_cam_tool::{TOOLS, ToolLibrary};
use kmb_core::{FeatureStatus, warnings_of};
use kmb_strategy_contour2d::CONTOUR;
use serde_json::json;

/// 指定径の工具で、モジュール 1・歯数 20 の歯車の外側を削った時の注意。
fn gear_contour_warnings(diameter: f64) -> Vec<String> {
    let mut b = DocBuilder::new();
    let gear = b.feature(
        "cad.gear@1",
        "歯車の輪郭",
        json!({"module":"1 mm", "teeth":"20", "pressure_angle":"20 deg"}),
    );
    let body = b.feature(
        "cad.gear_body@1",
        "歯車のボディ",
        json!({"gear":gear, "width":"4 mm"}),
    );
    let mut library = ToolLibrary::starter();
    library.tools.truncate(1);
    library.tools[0].diameter = diameter;
    library.tools[0].name = format!("平エンドミル {diameter} mm");
    let tool_number = library.tools[0].number;
    let tools = b.feature(TOOLS, "工具", library);
    let setup = b.feature(SETUP, "セットアップ", Setup::new(body));
    let contour = b.feature(
        CONTOUR,
        "歯の輪郭を削る",
        json!({
            "setup":setup, "tools":tools, "tool":tool_number,
            "chains":[{"sketch":gear}], "side":"outside",
            "stepdown":"4 mm"
        }),
    );
    let mut doc = b.finish("歯溝と工具の到達");
    let registry = kmb_bundle::registry();
    let kernel = kmb_bundle::kernel();
    let report = doc.recompute(&registry, kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(contour) else {
        panic!("輪郭加工が計算できる")
    };
    let (_, path) = toolpath_of(&outputs["toolpath"]).expect("輪郭加工の経路");
    assert!(
        !path.moves.is_empty(),
        "経路を作らないだけでは取り残しの通知にならない"
    );
    warnings_of(outputs)
}

#[test]
fn a_three_mm_tool_reports_unreachable_tooth_gaps_of_a_module_one_gear() {
    let warnings = gear_contour_warnings(3.0);
    eprintln!("径 3 mm の輪郭加工の注意: {warnings:?}");
    assert!(
        warnings.iter().any(|w| {
            w.contains("取残し")
                || w.contains("取り残し")
                || w.contains("削り残し")
                || w.contains("工具が入らない")
        }),
        "歯の間の底へ届かない工具を知らせる必要がある: {warnings:?}"
    );
}
