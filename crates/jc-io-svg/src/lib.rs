//! SVG の読み込みと書き出し、スケッチへの挿入と SVG での保存の操作。
//!
//! 読み込み([`from_svg`])はスケッチの読んだ形([`jc_sketch::Imported`])を返すだけで、スケッチへ
//! 入れるのはスケッチの [`jc_sketch::Sketch::insert_shapes`]。書き出し([`to_svg`])は、
//! レーザー加工機のソフト(LightBurn など)で同じ大きさで開ける形にする([`write`])。

mod commands;
mod svg;
pub mod write;

pub use commands::{EXPORT_SVG, INSERT_SVG, SvgCommands};
pub use svg::from_svg;
pub use write::{SvgOptions, to_svg};

/// この crate の名乗り(SVG の挿入と保存の操作)。組み立て(jc-bundle)が 1 回だけ並べる。
pub fn module() -> jc_core::Module {
    jc_core::Module::commands("jc-io-svg").with_extension(SvgCommands)
}

#[cfg(test)]
mod tests;
