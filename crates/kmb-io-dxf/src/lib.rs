//! DXF(ASCII・バイナリ)の読み込みと R2000 の書き出し、スケッチへの挿入と保存の操作。
//!
//! 読み込み([`from_dxf`])はスケッチの読んだ形([`kmb_sketch::Imported`])を返すだけで、スケッチへ
//! 入れるのはスケッチの [`kmb_sketch::Sketch::insert_shapes`]。

mod binary;
mod clean;
mod commands;
mod dxf;
mod r2000;

pub use binary::{from_dxf_bytes, from_dxf_bytes_with_layers};
pub use clean::{Cleaned, clean};
pub use commands::{DxfCommands, EXPORT_DXF, INSERT_DXF, LIST_DXF_LAYERS};
pub use dxf::{DxfLayer, DxfOptions, DxfRead, dxf_layers, from_dxf, from_dxf_with_layers, to_dxf};

/// この crate の名乗り(DXF の挿入と保存の操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::commands("kmb-io-dxf").with_extension(DxfCommands)
}

#[cfg(test)]
mod tests;
