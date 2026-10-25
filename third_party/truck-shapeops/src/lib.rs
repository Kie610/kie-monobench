//! Crate for operationg shapes. Provides boolean operations to Solid, and shape healing for importing shapes from other CAD systems.

// jisaku-cadcam: upstream denies every warning in release builds. crates.io dependencies are built
// with `--cap-lints`, so it never fired there; this copy is a path dependency and is not capped, and
// an unused item (`Alternative`) failed the release build. Warnings stay warnings here.
#![deny(clippy::all, rust_2018_idioms)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    unused_qualifications
)]

mod healing;
pub use healing::{RobustSplitClosedEdgesAndFaces, SplitClosedEdgesAndFaces};
mod transversal;
pub use transversal::{and, or, ShapeOpsCurve, ShapeOpsSurface};
mod alternative;
