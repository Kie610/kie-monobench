//! 確かめること: 平面の引数が基準平面の実際の横・縦・法線を説明する。

use kmb_core::{
    Context, Document, Extension, Kernel, NullKernel, Plane, RecomputeReport, Registry,
};
use std::sync::Arc;

#[test]
fn base_plane_arg_help_matches_actual_axes() {
    let doc = Document::new();
    let report = RecomputeReport::default();
    let registry = Registry::default();
    let kernel: Arc<dyn Kernel> = Arc::new(NullKernel);
    let context = Context::new(doc.data(), &report, &registry, &kernel);
    let actions = crate::DatumCommands.actions(&context);
    for id in [crate::ADD_DATUM_PLANE, crate::ADD_DATUM_AXIS] {
        let action = actions.iter().find(|a| a.id == id).unwrap();
        let help = &action.args.iter().find(|a| a.key == "plane").unwrap().help;
        for (plane, x, y, normal, text) in [
            (
                Plane::XY,
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                "xy: 横=X・縦=Y・法線=+Z",
            ),
            (
                Plane::YZ,
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                "yz: 横=Y・縦=Z・法線=+X",
            ),
            (
                Plane::ZX,
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                "zx: 横=Z・縦=X・法線=+Y",
            ),
        ] {
            assert_eq!(plane.x_axis, x);
            assert_eq!(plane.y_axis, y);
            assert_eq!(plane.normal(), normal);
            assert!(help.contains(text), "{id}: {help} に {text} がありません");
        }
    }
}
