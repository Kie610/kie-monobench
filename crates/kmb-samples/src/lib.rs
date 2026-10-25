//! 見本の文書。画面の `--sample`、CLI と MCP の `sample`、通しの試験が使う。
//! スケッチと文書を手早く組み立てる [`sample::SketchBuilder`]・[`sample::DocBuilder`] もここに置く
//! (CAD の通しの試験 kmb-solid と、画面の試験が使う)。
//!
//! 見本は CAD と CAM の両方を使うので、層をまたいでよいこの crate に置く
//! (CAM の crate が CAD の crate に依存しないように)。

pub mod cam;
mod edges;
pub mod sample;
mod sketch;
mod sweep;

pub use cam::{ReferenceCam, reference_cam, reference_cam_3d};
pub use edges::{
    corner_document, cylinder_cut_from_bottom_document, fillet_then_chamfer_document, look_document,
};
pub use sample::{ReferencePart, reference_part, reference_values};
pub use sketch::{sample_document, sketch_dims_document, sketch_tools_document};
pub use sweep::{SweepLoftSample, sweep_loft_document, sweep_loft_sample};

use kmb_core::Document;

/// 基準部品(穴あきの板)の文書。
pub fn reference_document() -> Document {
    reference_part().doc
}

/// 基準部品と、その 2.5D の加工の文書。
pub fn reference_cam_document() -> Document {
    reference_cam().part.doc
}

/// 面取り・フィレットつきの基準部品と、3D の仕上げを含む加工の文書。
pub fn reference_cam_3d_document() -> Document {
    reference_cam_3d().part.doc
}

#[cfg(test)]
mod air_tests;
#[cfg(test)]
mod cam_2d_tests;
#[cfg(test)]
mod cam_3d_tests;
#[cfg(test)]
mod cam_pick_tests;
#[cfg(test)]
mod cam_program_tests;
#[cfg(test)]
mod cam_reference_tests;
#[cfg(test)]
mod cam_rest_tests;
#[cfg(test)]
mod cam_tests;
#[cfg(test)]
mod fea_tests;
#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod hole_tests;
#[cfg(test)]
mod machine_tests;
#[cfg(test)]
mod matrix_tests;
#[cfg(test)]
mod trace_thread_tests;
