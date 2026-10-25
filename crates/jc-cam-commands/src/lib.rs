//! CAM の操作(画面・CLI・MCP に出す加工の追加・G-code・段取り表・シミュレーション・再生)。
//! セットアップごとのプログラムの組み立てと段取り表は jc-cam-program。
//!
//! | 種類 | 内容 |
//! | --- | --- |
//! | `cam.tools@2` | 工具ライブラリ(形と材料ごとの切削条件。`@1` も読める) |
//! | `cam.setup@1` | セットアップ(ボディ、ストック、作業座標系、安全高さ) |
//! | `cam.face@1` | フェイス加工 |
//! | `cam.contour2d@1` | 2D 輪郭加工 |
//! | `cam.chamfer2d@1` | V ビットでの縁の面取り |
//! | `cam.engrave@1` | スケッチの線をなぞる彫刻 |
//! | `cam.trace@1` | 選んだ辺かスケッチの線を、その高さのままなぞる(トレース) |
//! | `cam.pocket2d@1` | 2D ポケット加工 |
//! | `cam.adaptive2d@1` | 負荷制御粗取り(工具が材料に掛かる幅を一定に保つ) |
//! | `cam.drill@1` | 穴あけ(ドリル、ヘリカル) |
//! | `cam.thread@1` | ねじ切りミルで、選んだ穴かボスの壁にねじを切る |
//! | `cam.raster3d@1` | 3D の走査線加工(曲面、斜面の仕上げ) |
//! | `cam.waterline@1` | 3D の等高線加工(急な壁の仕上げ) |
//! | `cam.pencil3d@1` | 3D のペンシル加工(内側の角や谷をなぞる残り削り) |
//! | `cam.pattern@1` | 加工のパターン(元の加工の経路を格子・円・鏡写しで写す) |
//! | `cam.manual_nc@1` | 手動 NC(加工の間の停止・任意停止・コメント・停留・知らせ) |
//! | `cam.rough3d@1` | 3D の荒削り(段ごとのオフセットの輪。取残しも) |
//! | `cam.flat3d@1` | 3D の平らな所の仕上げ |
//! | `cam.spiral3d@1` | 3D の渦巻きの仕上げ |
//! | `cam.radial3d@1` | 3D の放射の仕上げ |
//!
//! 加工の機能はタイムラインで形状の後に並び、形状の寸法を変えると経路も作り直される。

pub use jc_cam_setup::{
    AirCuts, Fixture, Flip, HeightFrom, HeightsOutput, MANUAL_NC, ManualKind, ManualNc, MirrorAxis,
    OpBase, Origin, OriginZ, PATTERN, Pattern, PatternKind, SETUP, Setup, SetupInfo, Stock,
    StockMode, heights_of, toolpath_of,
};
pub use jc_cam_tool::{self as tool, TOOLS, TOOLS_V1, Tool, ToolKind, ToolLibrary};
pub use jc_post::{MachineProfile, Post};
pub use jc_sim::{HeadPart, MachineHit, MachineKind, MachineShape, SimOptions, SimReport, ZMap};
pub use jc_toolpath::{Manual, Program, Toolpath};

mod chains;
mod commands;
mod entry;
mod fixtures;
mod nc_program;
mod paths_cache;
mod picks;
mod playback;
mod reuse;
mod setup_ops;
mod stages;
mod tabs;
mod tool_order;
pub use chains::PICK_CHAINS;
pub use commands::{
    ADD_SETUP, CLEAR_STAGE, CamCommands, EXPORT_GCODE, EXPORT_GCODE_AS, MACHINE_SETTINGS,
    POST_SETTINGS, REST_POCKET, SETUP_SHEET, SHOW_STAGE, SIM_SETTINGS, SIMULATE, SIMULATE_GCODE,
    VIEW_GCODE,
};
pub use entry::{AUTO_ENTRY, PLACE_ENTRY};
pub use fixtures::{ADD_FIXTURE, CLEAR_FIXTURES};
pub use jc_cam_program::{
    ManualAt, PathSource, SetupPaths, carried_stock, chain_machine, chain_program, gcode,
    is_operation, machine_shape, path_cells, player, setup_sheet, setup_sheet_html, setups,
    simulate, simulate_chain, stage_shape, tool_changes, with_gcode_paths, world_mesh,
};
pub use nc_program::{
    ADD_MANUAL_NC, ADD_PATTERN, EXPORT_GCODE_ALL, EXPORT_GCODE_BY_TOOL, EXPORT_GCODE_SELECTED,
    SETUP_SHEET_HTML,
};
pub use playback::{
    EXPORT_SIM_VIDEO, EXPORT_STOCK, PLAY, PLAY_COLOR, PLAY_NEXT, PLAY_PREV, PLAY_SEEK, PLAY_STOP,
};
pub use reuse::{
    CLEAR_DEFAULTS, DEFAULTS_SETTINGS, DELETE_TEMPLATE, EXPORT_DEFAULTS, IMPORT_DEFAULTS,
    RESET_DEFAULTS, SAVE_DEFAULTS, SAVE_TEMPLATE, TEMPLATE_DIR, Template, TemplateOp, USE_TEMPLATE,
};
pub use tabs::PLACE_TABS;
pub use tool_order::ADD_TOOL;

/// 加工の操作のモジュール。足せる加工の戦略(各戦略の crate の `module()`)と、使うポストと機械は
/// 組み立て(jc-bundle)が決めて渡す。工具とセットアップの種類は jc-cam-tool・jc-cam-setup が登録する。
pub fn module(
    strategies: std::sync::Arc<[jc_cam_setup::Strategy]>,
    machines: jc_post::machines::MachineLibrary,
) -> jc_core::Module {
    jc_core::Module::commands("jc-cam-commands")
        .with_extension(CamCommands::new(machines, strategies))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_core::Registry;

    /// 試験で使う、全部の戦略の crate の名乗り(組み立ての jc-bundle と同じ並び)。
    fn strategy_modules() -> Vec<jc_cam_setup::StrategyModule> {
        vec![
            jc_strategy_face::module(),
            jc_strategy_contour2d::module(),
            jc_strategy_chamfer::module(),
            jc_strategy_engrave::module(),
            jc_strategy_laser::module(),
            jc_strategy_pocket2d::module(),
            jc_strategy_adaptive2d::module(),
            jc_strategy_drill::module(),
            jc_strategy_raster3d::module(),
            jc_strategy_waterline::module(),
            jc_strategy_pencil::module(),
            jc_strategy_rough3d::module(),
        ]
    }

    /// 試験で使う、加工の種類を全部登録した表。
    pub(crate) fn registry() -> Registry {
        let (mods, _) = jc_cam_setup::split(strategy_modules());
        let mut all = vec![
            jc_cam_tool::module(),
            jc_cam_setup::module(jc_post::machines::MachineLibrary::new(vec![
                jc_post_carvera_air::library_entry(),
            ])),
        ];
        all.extend(mods);
        jc_core::registry_of(&all)
    }

    /// 試験で使う、Carvera Air と全部の戦略の加工の操作。
    pub(crate) fn cam() -> CamCommands {
        CamCommands::new(
            jc_post::machines::MachineLibrary::new(vec![jc_post_carvera_air::library_entry()]),
            jc_cam_setup::split(strategy_modules()).1,
        )
    }

    pub(crate) fn info(flip: Flip) -> SetupInfo {
        SetupInfo {
            body: jc_core::ShapeHandle(1),
            origin: [0.0, 0.0, 5.0],
            stock_min: [0.0, 0.0, -5.0],
            stock_max: [10.0, 10.0, 0.0],
            safe_z: 5.0,
            retract_z: 1.0,
            clearance_z: 20.0,
            wcs: 1,
            material: String::new(),
            flip,
            flip_center: [5.0, 5.0, 2.5],
            tolerance: jc_cam_setup::MESH_TOLERANCE,
            keep_out: vec![],
            fixtures: vec![],
            rot: None,
            more_bodies: vec![],
            part_min: [0.0; 3],
            part_max: [10.0, 10.0, 0.0],
            stock_body: None,
            program: None,
            comment: String::new(),
            bed_offset: [0.0; 2],
            machine: jc_post_carvera_air::ID.into(),
            machine_values: jc_post_carvera_air::profile(),
            leveling: None,
        }
    }
}
