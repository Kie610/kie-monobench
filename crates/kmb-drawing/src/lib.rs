//! 文書の図面区画と、再計算した形から毎回作る用紙の線。

mod annotation;
mod annotation_commands;
mod commands;
pub mod export;
mod export_commands;
mod dimension;
mod import_dimensions;
mod lettering;
pub use annotation::*;
mod geometry;
mod paper;
mod picture;
pub use paper::{Title, TitleSource, TitleValue};
mod schema;
pub use picture::{sheet_picture, sheet_picture_with_lines};
pub use schema::*;

pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-drawing", |r| r.declare_section::<Drawings>())
        .with_extension(commands::DrawingCommands)
        .with_extension(export_commands::ExportCommands)
        .with_extension(annotation_commands::AnnotationCommands::default())
}
