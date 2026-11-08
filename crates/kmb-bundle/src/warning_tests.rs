//! 確かめること: 同じ歯車の型付き注意が status と NC の書き出しへ一度だけ届く。
use kmb_cam_setup::{SETUP, Setup};
use kmb_cam_tool::{TOOLS, ToolLibrary};
use kmb_samples::sample::DocBuilder;
use kmb_strategy_contour2d::CONTOUR;
use serde_json::json;
fn gear_contour(diameter: f64) -> (kmb_core::Document, kmb_core::FeatureId, kmb_core::FeatureId) {
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
    (b.finish("歯溝と工具の到達"), setup, contour)
}

#[test]
fn typed_clearance_reaches_headless_status_and_each_nc_export_once() {
    let (doc, setup, contour) = gear_contour(3.0);
    let mut headless = kmb_session::Headless::new(crate::modules());
    headless.replace(doc, None);
    let status = headless.status();
    let feature = status["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == contour.0)
        .unwrap();
    let typed = &feature["warning_messages"];
    assert_eq!(typed.as_array().unwrap().len(), 1);
    assert_eq!(typed[0]["id"], "feature-tool-clearance-warning");
    assert_eq!(typed[0]["args"]["count"], 20.0);
    assert_eq!(typed[0]["args"]["diameter"], 0.76);
    assert!(
        !feature["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().contains("工具が届かない"))
    );
    for (action, selected) in [
        (kmb_cam_commands::EXPORT_GCODE, setup),
        (kmb_cam_commands::EXPORT_GCODE_SELECTED, contour),
        (kmb_cam_commands::EXPORT_GCODE_ALL, setup),
        (kmb_cam_commands::EXPORT_GCODE_BY_TOOL, setup),
    ] {
        let result = headless
            .run(action, Some(selected), None, &serde_json::Value::Null)
            .unwrap();
        let kmb_session::Ran::File { contents, info, .. } = result else {
            panic!("NC ファイルを返す: {result:?}")
        };
        assert!(!contents.is_empty());
        assert_eq!(&info["warning_messages"], typed, "{action}: {info}");
        assert_eq!(
            kmb_core::warnings_from_info_with(&info, |_| None)
                .iter()
                .filter(|w| w.contains("工具が届かない"))
                .count(),
            1
        );
    }
}
