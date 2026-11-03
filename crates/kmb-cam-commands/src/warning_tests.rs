//! 確かめること: 区画ごとの NC ファイルにも、経路の持ち主の注意が重複せず届く。
use super::*;
use kmb_cam_setup::sections::{Rect2, Section, SectionKind, SectionOutput, Sections};
use kmb_core::{Context, FeatureId, FeatureResult, FeatureStatus, Outputs};

#[test]
fn section_files_carry_one_notice_even_with_repeated_path_sources() {
    let id = FeatureId(7);
    let mut outputs = Outputs::new();
    kmb_core::add_warnings(&mut outputs, vec!["旧形式の注意".into()]);
    kmb_core::add_warning_messages(
        &mut outputs,
        vec![kmb_core::WarningMessage {
            id: "example-narrow-region".into(),
            args: [("count".into(), kmb_core::WarningArg::Number(2.0))].into(),
            fallback: "工具を確かめてください".into(),
        }],
    );
    let mut report = kmb_core::RecomputeReport::default();
    report.features = vec![FeatureResult {
        id,
        name: "加工".into(),
        status: FeatureStatus::Ok {
            outputs,
            reused: false,
        },
    }];
    let data = kmb_core::DocumentData::default();
    let registry = tests::registry();
    let kernel: std::sync::Arc<dyn kmb_core::Kernel> = std::sync::Arc::new(kmb_core::NullKernel);
    let cx = Context::new(&data, &report, &registry, &kernel);
    let mut info = tests::info(Flip::None);
    let window = Rect2 {
        min: [0.0, 0.0],
        max: [10.0, 10.0],
    };
    info.sections = Some(Sections {
        kind: SectionKind::Sheets,
        output: SectionOutput::Files,
        list: (1..=2)
            .map(|index| Section {
                index,
                label: format!("板 {index}"),
                cut: window,
                own: window,
                shift: [0.0; 2],
                stock_min: [0.0, 0.0, -5.0],
                stock_max: [10.0, 10.0, 0.0],
            })
            .collect(),
    });
    let path = Toolpath {
        name: "加工".into(),
        tool: ToolLibrary::starter().tools[0].clone(),
        rpm: 10000.0,
        safe_z: 5.0,
        retract_z: 1.0,
        edge_break: 0.0,
        dwells: vec![],
        spans: vec![],
        links: Default::default(),
        moves: vec![
            kmb_toolpath::Move::Rapid {
                to: [1.0, 1.0, 5.0],
            },
            kmb_toolpath::Move::Linear {
                to: [1.0, 1.0, -1.0],
                feed: 100.0,
            },
            kmb_toolpath::Move::Linear {
                to: [2.0, 1.0, -1.0],
                feed: 100.0,
            },
            kmb_toolpath::Move::Rapid {
                to: [2.0, 1.0, 5.0],
            },
        ],
    };
    let source = PathSource {
        id,
        type_id: "example.operation@1".into(),
        rest: false,
        cutting: None,
        rough_grid: false,
        cuts_free: false,
        skip_air: false,
        original: None,
        section_fixed: false,
        section_entry: None,
    };
    let paths = SetupPaths {
        setup: FeatureId(3),
        name: "セットアップ".into(),
        info,
        paths: vec![path.clone(), path],
        sources: vec![source.clone(), source],
        manual: vec![],
        failed: vec![],
        part: vec![],
        air_start: None,
        nest: None,
        nest_names: Default::default(),
    };
    let outcome = tests::cam().section_files(&cx, &paths).unwrap();
    let kmb_core::Outcome::Files { files, info } = outcome else {
        panic!("区画ごとのファイル")
    };
    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|(_, bytes)| !bytes.is_empty()));
    assert_eq!(info["warning_messages"].as_array().unwrap().len(), 1);
    assert_eq!(info["warnings"], serde_json::json!(["旧形式の注意"]));
    assert_eq!(
        kmb_core::warnings_from_info_with(&info, |_| None),
        ["旧形式の注意", "工具を確かめてください"]
    );
}
