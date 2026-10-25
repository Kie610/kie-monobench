//! 組み立て。どの機能のモジュールを使うかは、ここだけで決める。
//!
//! 画面(jc-app)、CLI(jc-cli)、MCP(jc-mcp)はここから登録表とカーネルを受け取るので、
//! 機能を足すときは、その crate の `register` をここに 1 行足すだけでよい。

use jc_core::{Kernel, Module, Modules, Registry};
use std::sync::Arc;

/// 加工の戦略の crate の名乗り。ツールバーの並びは各戦略が名乗る(ここの順に依らない)。
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

/// 加工の戦略の一覧(各戦略の crate の名乗り)。
pub fn strategies() -> Arc<[jc_cam_setup::Strategy]> {
    jc_cam_setup::split(strategy_modules()).1
}

/// マシンライブラリ(機械の値とポストの対)。最初の機械が新しいセットアップの既定。
pub fn machines() -> jc_post::machines::MachineLibrary {
    let mut v = vec![jc_post_carvera_air::library_entry()];
    v.extend(jc_post::machines::MachineLibrary::generic());
    // ポストのプラグイン(WASM。wasmtime を引くので、加工の操作は読み手を知らずに受け取る)
    jc_post::machines::MachineLibrary::new(v).with_loader(std::sync::Arc::new(|path| {
        Ok(Box::new(jc_plugin_host::WasmPost::from_file(path)?) as Box<dyn jc_post::Post>)
    }))
}

/// 使うモジュールの一覧(1 つずつ)。機能を足すときは、その crate の `module()` をここに 1 行足す。
/// 操作は、この順に画面へ並ぶ。
pub fn all() -> Vec<Module> {
    let (cam, strategies) = jc_cam_setup::split(strategy_modules());
    let machines = machines();
    let mut v = vec![
        // STEP の読み込みは設計の「挿入」の先頭(SVG の挿入より前。Kie 2026-10-21 の画板)。
        // 挿入の操作はスケッチのモジュールの中へ並べ直すので、スケッチより前に置く
        jc_cad_import_step::module(),
        jc_sketch::module(),
        jc_sketch_3d::module(),
        jc_text::module(),
        jc_io_dxf::module(),
        jc_io_svg::module(),
        jc_cad_extrude::module(),
        jc_cad_hole::module(),
        jc_cad_revolve::module(),
        jc_cad_sweep::module(),
        jc_cad_boolean::module(),
        jc_cad::module(),
        jc_cad_edge::module(),
        jc_cad_pattern::module(),
        jc_cad_modify::module(),
        jc_sketch_canvas::module(),
        jc_scene::module(),
        jc_config::module(),
        jc_asm::module(),
        jc_io_stl::module(),
        jc_io_view::module(),
        jc_io_render::module(),
        jc_fea::module(),
        jc_flow::module(),
        jc_study::module(),
        jc_cam_tool::module(),
        jc_cam_setup::module(machines.clone()),
    ];
    v.extend(cam);
    v.push(jc_cam_commands::module(strategies, machines));
    v
}

/// すべての機能の種類を登録した表。
pub fn registry() -> Registry {
    jc_core::registry_of(&all())
}

/// 形状カーネルの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelKind {
    /// SMLib(usd-brep)。既定。STEP の読み込みだけ truck に任せる
    Smlib,
    /// truck(以前のカーネル。足し引きが遅い)
    Truck,
}

impl KernelKind {
    /// 環境変数 `JC_KERNEL`(`smlib` か `truck`)で選ぶ。無ければ既定(SMLib を入れて
    /// ビルドしていれば SMLib、外していれば truck)。知らない名前は既定にする。
    pub fn from_env() -> KernelKind {
        match std::env::var("JC_KERNEL").ok().as_deref().map(str::trim) {
            Some(n) if n.eq_ignore_ascii_case("truck") => KernelKind::Truck,
            Some(n) if n.eq_ignore_ascii_case("smlib") && cfg!(feature = "smlib") => {
                KernelKind::Smlib
            }
            _ => KernelKind::default(),
        }
    }
}

impl Default for KernelKind {
    fn default() -> Self {
        if cfg!(feature = "smlib") {
            KernelKind::Smlib
        } else {
            KernelKind::Truck
        }
    }
}

/// 指定した種類の形状カーネル。
pub fn kernel_of(kind: KernelKind) -> Arc<dyn Kernel> {
    match kind {
        #[cfg(feature = "smlib")]
        KernelKind::Smlib => Arc::new(
            jc_kernel_smlib::SmlibKernel::new()
                .with_step_import(jc_kernel_truck::TruckKernel::new()),
        ),
        #[cfg(not(feature = "smlib"))]
        KernelKind::Smlib => Arc::new(jc_kernel_truck::TruckKernel::new()),
        KernelKind::Truck => Arc::new(jc_kernel_truck::TruckKernel::new()),
    }
}

/// 使う形状カーネル([`KernelKind::from_env`] で選ぶ)。
pub fn kernel() -> Arc<dyn Kernel> {
    let kind = KernelKind::from_env();
    log::info!("形状カーネル: {kind:?}");
    kernel_of(kind)
}

/// 登録表、カーネル、モジュールの操作の一式(画面・CLI・MCP に渡す)。
pub fn modules() -> Modules {
    Modules::assemble(all(), kernel())
}

/// 見本の文書の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sample {
    /// 基準部品(穴あきの板)
    Part,
    /// 基準部品と 2.5D の加工
    Cam,
    /// 面取り・フィレットつきの基準部品と 3D の仕上げ
    Cam3d,
}

impl Sample {
    pub const ALL: [Sample; 3] = [Sample::Part, Sample::Cam, Sample::Cam3d];

    pub fn label(self) -> &'static str {
        match self {
            Sample::Part => "基準部品",
            Sample::Cam => "基準部品と 2.5D の加工",
            Sample::Cam3d => "基準部品と 3D の仕上げ",
        }
    }

    /// 文書を作る関数。
    pub fn make(self) -> fn() -> jc_core::Document {
        match self {
            Sample::Part => jc_samples::reference_document,
            Sample::Cam => jc_samples::reference_cam_document,
            Sample::Cam3d => jc_samples::reference_cam_3d_document,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_core::{FieldKind, category};

    const KNOWN: &[&str] = &[
        category::SKETCH,
        category::SOLID,
        category::TOOLS,
        category::SETUP,
        category::OPERATION,
        category::STUDY,
        category::DEBUG,
    ];

    // 同じ登録簿を 1 回だけ作って確かめる:
    // - 登録した種類はすべて、知っている区分と表示名と欄の説明を持つ
    // - 加工の種類は区分で見つかり、セットアップは加工に数えない
    #[test]
    fn every_registered_type_is_described_and_operations_are_found_by_category() {
        let r = registry();
        for d in r.descriptors() {
            assert!(
                KNOWN.contains(&d.category.as_str()),
                "{} の区分 {} は未知",
                d.type_id,
                d.category
            );
            assert!(
                !d.label.is_empty() && d.label != d.type_id,
                "{} に表示名がない",
                d.type_id
            );
            for f in &d.fields {
                assert!(
                    !f.key.is_empty() && !f.label.is_empty(),
                    "{}: {f:?}",
                    d.type_id
                );
                if let FieldKind::Choice { choices } = &f.kind {
                    assert!(!choices.is_empty(), "{}: {}", d.type_id, f.key);
                }
            }
        }

        let ops: Vec<&str> = r.in_category(category::OPERATION).collect();
        for t in [
            jc_strategy_face::FACE,
            jc_strategy_contour2d::CONTOUR,
            jc_strategy_pocket2d::POCKET,
            jc_strategy_drill::DRILL,
            jc_strategy_raster3d::RASTER3D,
            jc_strategy_waterline::WATERLINE,
            jc_strategy_pencil::PENCIL,
            jc_strategy_rough3d::ROUGH3D,
            jc_strategy_rough3d::FLAT3D,
            jc_strategy_raster3d::SPIRAL3D,
            jc_strategy_raster3d::RADIAL3D,
        ] {
            assert!(ops.contains(&t), "{t}");
        }
        assert!(!jc_cam_commands::is_operation(&r, jc_cam_commands::SETUP));
    }

    #[test]
    fn the_kernel_is_chosen_by_name() {
        assert_eq!(kernel_of(KernelKind::Truck).name(), "truck");
        let default = kernel_of(KernelKind::default());
        if cfg!(feature = "smlib") {
            assert_eq!(default.name(), "smlib");
        } else {
            assert_eq!(default.name(), "truck");
        }
    }
}
