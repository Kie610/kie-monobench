//! DXF(ASCII)の読み込みと書き出し、スケッチへの挿入とスケッチの DXF での保存の操作。
//!
//! 読み込み([`from_dxf`])はスケッチの読んだ形([`jc_sketch::Imported`])を返すだけで、スケッチへ
//! 入れるのはスケッチの [`jc_sketch::Sketch::insert_shapes`]。

mod commands;
mod dxf;

pub use commands::{DxfCommands, EXPORT_DXF, INSERT_DXF};
pub use dxf::{DxfOptions, from_dxf, to_dxf};

/// この crate の名乗り(DXF の挿入と保存の操作)。組み立て(jc-bundle)が 1 回だけ並べる。
pub fn module() -> jc_core::Module {
    jc_core::Module::commands("jc-io-dxf").with_extension(DxfCommands)
}

#[cfg(test)]
mod tests;
