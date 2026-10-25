//! スイープとロフト(M7)の見本: 断面の円と L の字の経路、ロフトの四角と、その上 30 mm の円。

use crate::sample::{DocBuilder, SketchBuilder};
use jc_core::Document;
use jc_sketch::SketchPlane;

/// 見本の各スケッチの番号。
pub struct SweepLoftSample {
    pub doc: Document,
    /// XY 平面の、原点を中心にした直径 6 の円
    pub profile: jc_core::FeatureId,
    /// ZX 平面の L の字(原点から上へ `length`、そこから x へ 30)
    pub path: jc_core::FeatureId,
    /// XY 平面の、(50, -10) から 20 × 20 の四角
    pub square: jc_core::FeatureId,
    /// XY 平面から 30 mm 上の、(60, 0) を中心にした直径 16 の円
    pub circle: jc_core::FeatureId,
}

pub fn sweep_loft_sample() -> SweepLoftSample {
    let mut b = DocBuilder::new();
    b.param("length", "40 mm");
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let c = s.fixed_point(0.0, 0.0);
    s.circle(c, "6 mm", 3.0);
    let profile = b.feature(jc_sketch::SKETCH, "断面", s.build());
    // ZX 平面の座標は (z, x)
    let mut s = SketchBuilder::new(SketchPlane::Zx);
    let o = s.fixed_point(0.0, 0.0);
    let p1 = s.point_from(o, "length", "0 mm", [40.0, 0.0]);
    let p2 = s.point_from(p1, "0 mm", "30 mm", [40.0, 30.0]);
    s.line(o, p1);
    s.line(p1, p2);
    let path = b.feature(jc_sketch::SKETCH, "経路", s.build());
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let corner = s.fixed_point(50.0, -10.0);
    s.rectangle(corner, "20 mm", "20 mm", [50.0, -10.0, 20.0, 20.0]);
    let square = b.feature(jc_sketch::SKETCH, "下の四角", s.build());
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let c = s.fixed_point(60.0, 0.0);
    s.circle(c, "16 mm", 8.0);
    let mut top = s.build();
    top.offset = Some("30 mm".into());
    let circle = b.feature(jc_sketch::SKETCH, "上の円", top);
    SweepLoftSample {
        doc: b.finish("スイープとロフトの見本"),
        profile,
        path,
        square,
        circle,
    }
}

/// [`sweep_loft_sample`] の文書(画面の台本の `#! sample sweep`)。
pub fn sweep_loft_document() -> Document {
    sweep_loft_sample().doc
}
