//! 辺の丸めと面取りの見本(画面で形を確かめる)。

use crate::sample::{DocBuilder, SketchBuilder};
use kmb_cad::{Direction, Operation};
use kmb_cad_edge::{CHAMFER, FILLET};
use kmb_cad_extrude::{EXTRUDE, Extrude};
use kmb_core::{Document, FeatureId};
use kmb_sketch::SketchPlane;
use serde_json::json;

/// 箱(x 0〜20、y 0〜20、高さ 10)を作り、その ID を返す。
fn plain_box(b: &mut DocBuilder) -> FeatureId {
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "20 mm", "20 mm", [0.0, 0.0, 20.0, 20.0]);
    let sketch = b.feature(kmb_sketch::SKETCH, "箱の輪郭", s.build());
    b.feature(
        EXTRUDE,
        "箱",
        Extrude {
            profile: Some(sketch),
            distance: "10 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            regions: Vec::new(),
            ..Default::default()
        },
    )
}

/// 箱の上面の手前の辺を半径 4 mm で丸め、続けて上面の右の辺を 2 mm 面取りした文書
/// (面取りが丸めの端を通り抜けるか)。
pub fn fillet_then_chamfer_document() -> Document {
    let mut b = DocBuilder::new();
    let p = plain_box(&mut b);
    // 輪郭の線は下(手前)e5、右 e6、上(奥)e7、左 e8
    let f = |n: &str| format!("f{}:{n}", p.0);
    let r = b.feature(
        FILLET,
        "フィレット1",
        json!({"target": p, "edges": [[f("end"), f("side:e5")]], "radius": "4 mm"}),
    );
    b.feature(
        CHAMFER,
        "面取り1",
        json!({"target": r, "edges": [[f("end"), f("side:e6")]], "distance": "2 mm"}),
    );
    b.finish("丸めの後の面取り")
}

/// 箱の手前右上の角に集まる 3 本の辺を、`type_id`(フィレットか面取り)で角の形 `corner`
/// (`miter`・`ball`・`patch`)にした文書。
pub fn corner_document(type_id: &str, corner: &str) -> Document {
    let mut b = DocBuilder::new();
    let p = plain_box(&mut b);
    let f = |n: &str| format!("f{}:{n}", p.0);
    let mut data = json!({
        "target": p,
        "edges": [[f("end"), f("side:e5")], [f("end"), f("side:e6")], [f("side:e5"), f("side:e6")]],
        "corner": corner,
    });
    if type_id == FILLET {
        data["radius"] = json!("4 mm");
    } else {
        data["distance"] = json!("3 mm");
    }
    let name = if type_id == FILLET {
        "フィレット1"
    } else {
        "面取り1"
    };
    b.feature(type_id, name, data);
    b.finish("3 本の辺の角")
}

/// 円柱(径 10 mm、高さ 20 mm)を、底面のスケッチの帯(幅 4 mm、円柱より長い)で上まで切り取って
/// 2 つに分けた文書(切り口で側面が分かれた所に、不要な線が出ないかを見る)。
pub fn cylinder_cut_from_bottom_document() -> Document {
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.circle(o, "10 mm", 5.0);
    let sketch = b.feature(kmb_sketch::SKETCH, "円柱の輪郭", s.build());
    let cylinder = b.feature(
        EXTRUDE,
        "円柱",
        Extrude {
            profile: Some(sketch),
            distance: "20 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            regions: Vec::new(),
            ..Default::default()
        },
    );
    // 底面の座標は、x が世界の x、y が世界の -y
    let mut s = SketchBuilder::new(SketchPlane::face(
        cylinder,
        format!("f{}:start", cylinder.0),
    ));
    let c = s.fixed_point(-2.0, -8.0);
    s.rectangle(c, "4 mm", "16 mm", [-2.0, -8.0, 4.0, 16.0]);
    let band = b.feature(kmb_sketch::SKETCH, "帯の輪郭", s.build());
    b.feature(
        EXTRUDE,
        "帯の切り取り",
        Extrude {
            profile: Some(band),
            distance: "10 mm".into(),
            direction: Direction::Reverse,
            distance2: None,
            extent: kmb_cad::Extent::All,
            to_face: None,
            to_offset: None,
            operation: Operation::Cut,
            target: Some(cylinder),
            regions: Vec::new(),
            ..Default::default()
        },
    );
    b.finish("円柱を底面から切り取る見本")
}

/// 映り込みを見る見本: 板(120×45×10)の真ん中に、上の縁と根元を半径 1.5 mm で丸めた丸い突起
/// (直径 12、高さ 7)を立て、原点に輪(太さ 20、中心の半径 24)を置いた文書。
/// 曲面(フィレット・輪)の映り込み、平らな面の濃淡、稜線との兼ね合いを画面で確かめる。
pub fn look_document() -> Document {
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(-145.0, -22.5);
    s.rectangle(o, "120 mm", "45 mm", [-145.0, -22.5, 120.0, 45.0]);
    let sketch = b.feature(kmb_sketch::SKETCH, "板の輪郭", s.build());
    let extrude = |b: &mut DocBuilder, name: &str, profile, distance: &str, operation, target| {
        b.feature(
            EXTRUDE,
            name,
            Extrude {
                profile: Some(profile),
                distance: distance.into(),
                direction: Direction::Normal,
                operation,
                target,
                ..Default::default()
            },
        )
    };
    let plate = extrude(&mut b, "板", sketch, "10 mm", Operation::New, None);
    // 突起は床から立てて板と合わせる(上の面から 7 mm 出る)
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let c = s.fixed_point(-85.0, 0.0);
    s.circle(c, "12 mm", 6.0);
    let sketch = b.feature(kmb_sketch::SKETCH, "突起の輪郭", s.build());
    let boss = extrude(
        &mut b,
        "突起",
        sketch,
        "17 mm",
        Operation::Join,
        Some(plate),
    );
    let f = |id: FeatureId, n: &str| format!("f{}:{n}", id.0);
    let rounded = b.feature(
        FILLET,
        "突起の丸め",
        json!({
            "target": boss,
            "edges": [[f(boss, "end"), f(boss, "side:e2")], [f(boss, "side:e2"), f(plate, "end")]],
            "radius": "1.5 mm",
        }),
    );
    let _ = rounded;
    let mut p = SketchBuilder::new(SketchPlane::Zx);
    let o = p.fixed_point(0.0, 0.0);
    let c = p.point_from(o, "24 mm", "10 mm", [24.0, 10.0]);
    p.circle(c, "20 mm", 10.0);
    let profile = b.feature(kmb_sketch::SKETCH, "輪の断面", p.build());
    b.feature(
        "solid.revolve@1",
        "輪",
        json!({"profile": profile, "axis": {"kind": "origin_z"}, "angle": "360 deg"}),
    );
    b.finish("映り込みの見本")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutting_a_cylinder_from_its_bottom_draws_no_line_on_the_side_but_the_cut_corners() {
        // まとめてレビュー 2026-10-18: 円柱の底面からスケッチで切り取ると、側面に不要な線が出た
        // (側面が 2 つの面に分かれ、元の継ぎ目の所が面の境になった)。描くのは切り口の角の 4 本だけ
        let m = kmb_bundle::modules();
        let mut doc = cylinder_cut_from_bottom_document();
        let report = doc.recompute(&m.registry, m.kernel.as_ref());
        assert_eq!(report.error_count(), 0, "{report:?}");
        let bodies = kmb_core::visible_bodies(doc.data(), &m.registry, &report);
        let mut vertical = Vec::new();
        for (_, h) in bodies {
            let mesh = m.kernel.mesh(h, 0.05).unwrap();
            for e in &mesh.edges {
                let on_side = e
                    .iter()
                    .all(|p| ((p[0] * p[0] + p[1] * p[1]).sqrt() - 5.0).abs() < 1e-3);
                let (lo, hi) = e
                    .iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), p| {
                        (a.min(p[2]), b.max(p[2]))
                    });
                if on_side && hi - lo > 1e-3 {
                    vertical.push(e[0]);
                }
            }
        }
        assert_eq!(vertical.len(), 4, "{vertical:?}");
        assert!(
            vertical.iter().all(|p| (p[0].abs() - 2.0).abs() < 1e-3),
            "切り口の角(x = ±2)だけ: {vertical:?}"
        );
    }
}
