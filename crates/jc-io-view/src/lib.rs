//! 今の視点から見えるボディの線を 2D の DXF([`view_dxf`])と SVG([`view_svg`])に書き出す
//! (稜線と曲面の輪郭、隠れ線は破線の画層に分けられる)。書き出しの操作は [`ViewCommands`]。

mod commands;
pub mod view_dxf;
pub mod view_svg;
pub use commands::{EXPORT_VIEW_DXF, EXPORT_VIEW_DXF_HIDDEN, EXPORT_VIEW_SVG, ViewCommands};

/// この crate の名乗り(操作)。組み立て(jc-bundle)が 1 回だけ並べる。
pub fn module() -> jc_core::Module {
    jc_core::Module::commands("jc-io-view").with_extension(ViewCommands)
}
