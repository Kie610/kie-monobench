//! CAD の機能を全部通した試験(試験だけの crate。ほかの crate からは使わない)。
//!
//! 押し出し・回転・ブーリアン・穴・パターン・辺・シェル・STEP の読み込みを、同じ文書と
//! 実際のカーネルで通して確かめる。機能はそれぞれ別の crate にあり、組み立て(kmb-bundle)は
//! 各 crate を直接登録する。見本(基準部品とスケッチの組み立て)は kmb-samples の `sample`。
//!
//! | 種類 | crate | 内容 |
//! | --- | --- | --- |
//! | `solid.extrude@1` | kmb-cad-extrude | スケッチの輪郭を押し出す。新しいボディ、結合、切り取り、交差 |
//! | `solid.revolve@1` | kmb-cad-revolve | スケッチの輪郭を軸まわりに回す |
//! | `solid.sweep@1` / `solid.loft@1` | kmb-cad-sweep | 輪郭を経路に沿って動かす、断面をつなぐ |
//! | `solid.boolean@1` | kmb-cad-boolean | 2 つのボディの結合、切り取り、交差 |
//! | `solid.hole@1` | kmb-cad-hole | スケッチの点に、指定の径と深さ(または貫通)の穴をあける |
//! | `solid.thread@1` | kmb-cad-hole | 円筒の面にメートルねじの属性を付ける(形は変えない) |
//! | `solid.linear_pattern@1` / `solid.circular_pattern@1` / `solid.mirror@1` | kmb-cad-pattern | 機能の道具(押し出しや穴の形)を動かして同じ演算をくり返す |
//! | `solid.fillet@1` / `solid.chamfer@1` | kmb-cad-edge | 2 つの平面の境目のまっすぐな辺(出っ張りもへこみも)を丸める、面取りする |
//! | `solid.import_step@1` | kmb-cad-import-step | STEP の形を読み込む(中身は文書に入れて持つ) |
//!
//! 押し出し、回転、穴は、道具の作り方(kmb-cad の `Recipe`)を `recipe` にも出す。
//! パターンとミラーはこれを動かして道具を作り直す。

#![cfg(test)]

pub use kmb_cad::{Direction, EXTENSIONS, Extent, Operation, RECIPE_TYPE, Recipe, extend_flush};
pub use kmb_cad_boolean::{BOOLEAN, BooleanFeature};
pub use kmb_cad_edge::{
    CHAMFER, CHAMFER_FACE, Chamfer, ChamferRow, Corner, EdgeCommands, EdgeRow, EdgeSelection,
    FILLET, FILLET_FACE, Fillet, FilletRow, SHELL, Shell, ShellDirection,
};
pub use kmb_cad_extrude::{
    EXTRUDE, EXTRUDE_SKETCH, EXTRUDE_TO_FACE, Extrude, ExtrudeCommands, Source, Start, Thin, ToFace,
};
pub use kmb_cad_hole::{
    Fit, HOLE, HOLE_TO_FACE, Hole, HoleExtent, HoleKind, HoleStop, THREAD, THREAD_ON_FACE, Tap,
    Thread, Tip, thread,
};
pub use kmb_cad_import_step::{IMPORT_STEP, IMPORT_STEP_FILE, ImportStep, ImportStepCommands};
pub use kmb_cad_pattern::{
    CIRCULAR_PATTERN, CircularPattern, LINEAR_PATTERN, LinearPattern, MIRROR, MIRROR_ON_FACE,
    Mirror, MirrorCommands, PATTERN_AXIS, PatternKind, PlaneRef,
};
pub use kmb_cad_revolve::{
    Axis, REVOLVE, REVOLVE_AXIS, REVOLVE_SKETCH, REVOLVE_TO_FACE, Revolve, RevolveExtent,
    RevolveStop,
};

use kmb_core::Registry;
pub use kmb_core::visible_bodies;

/// ソリッドの機能をすべて登録する(試験用。組み立ては各 crate を直接登録する)。
pub fn register(reg: &mut Registry) {
    kmb_cad_extrude::register(reg);
    kmb_cad_revolve::register(reg);
    kmb_cad_sweep::register(reg);
    kmb_cad_surface::register(reg);
    kmb_cad_boolean::register(reg);
    kmb_cad_hole::register(reg);
    kmb_cad_pattern::register(reg);
    kmb_cad_modify::register(reg);
    kmb_asm::register(reg);
    kmb_cad_edge::register(reg);
    kmb_cad_import_step::register(reg);
}

mod adopted_failure_tests;
mod adopted_io_tests;
mod adopted_reference_tests;
mod adopted_shape_tests;
mod adopted_sketch_tests;
mod asm_tests;
mod datum_tests;
mod edge_tests;
mod extrude_revolve_hole_tests;
mod face_edit_tests;
mod face_extrude_tests;
mod face_name_tests;
mod face_revolve_tests;
mod fastener_tests;
mod fillet_material_tests;
mod fillet_rows_tests;
mod hole_tests;
#[cfg(test)]
mod link_tests;
mod modify_tests;
mod multi_pick_tests;
mod occurrence_tests;
mod origin_tests;
mod pattern_tests;
mod point_loft_tests;
mod segment_tests;
mod shell_tests;
mod surface_tests;
mod sweep_loft_tests;
mod tests;
mod text_outline_tests;
mod text_tests;
