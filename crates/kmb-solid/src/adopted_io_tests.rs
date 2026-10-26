//! 他のソフトで起きた読み書きの不具合が、JC では起きないことを確かめる(採用分の点検で「試験だけ」と
//! 判定した物)。ねじ穴のある部品の STEP の書き出し、細い面のある形の網、読み込んだ STEP の面からの
//! 押し出し。

use super::*;
use crate::tests::{TestKernel, assert_ok, close, final_mass, rect_sketch, registry};
use kmb_core::{Document, ShapeHandle};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;

/// 見えているボディ 1 つ。
fn body(doc: &mut Document, k: &TestKernel) -> ShapeHandle {
    let reg = registry();
    let report = doc.recompute(&reg, &**k);
    assert_ok(&report);
    let bodies = visible_bodies(doc.data(), &reg, &report);
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    bodies[0].1
}

fn plate(b: &mut DocBuilder, [x, y, w, h]: [f64; 4], t: f64) -> kmb_core::FeatureId {
    let s = rect_sketch(b, SketchPlane::Xy, x, y, w, h);
    b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(s),
            distance: format!("{t} mm"),
            ..Default::default()
        },
    )
}

/// 10-20 p5: ねじ穴(M6 のめねじ、貫通)を 2 つ開けた板を STEP に書き出すと止まらずに書け、
/// 読み直すと体積・外形・面の数が元と同じ(JC はねじ山を形にせず下穴の円柱で書く)。
#[test]
fn a_plate_with_tapped_holes_writes_to_step_and_reads_back() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!(
            "truck が書いた STEP は、読み込みの網づくりが 60 秒で終わらない(SMLib が書いた STEP は同じ読み込みで読める)"
        );
    }
    let mut b = DocBuilder::new();
    let p = plate(&mut b, [0.0, 0.0, 30.0, 20.0], 10.0);
    let mut s = SketchBuilder::new(SketchPlane::face(p, format!("f{}:end", p.0)));
    let o = s.fixed_point(0.0, 0.0);
    let points = [[8.0, 10.0], [22.0, 10.0]]
        .map(|q| {
            let c = s.point_from(o, &format!("{} mm", q[0]), &format!("{} mm", q[1]), q);
            kmb_core::PointId(c.0)
        })
        .to_vec();
    let hs = b.feature(kmb_sketch::SKETCH, "穴の位置", s.build());
    b.feature(
        HOLE,
        "ねじ穴",
        Hole {
            points,
            tap: Tap::Tapped,
            size: Some("M6".into()),
            ..Hole::new(p, hs)
        },
    );
    let mut doc = b.finish("ねじ穴の板");
    let h = body(&mut doc, &k);
    let before = k.mass(h).unwrap();
    // M6 の下穴は径 5
    let drilled = 6000.0 - 2.0 * std::f64::consts::PI * 2.5 * 2.5 * 10.0;
    assert!(
        close(before.volume, drilled, 1e-3),
        "下穴 {} vs {drilled}",
        before.volume
    );
    let step = k.export_step(&[h]).unwrap();
    let back = k.import_step(&step).unwrap();
    let after = k.mass(back).unwrap();
    // truck は網で読むので、円柱の折れ線の分だけずれる
    assert!(
        close(after.volume, before.volume, 2e-3),
        "{} vs {}",
        after.volume,
        before.volume
    );
    for i in 0..3 {
        assert!((after.min[i] - before.min[i]).abs() < 1e-3);
        assert!((after.max[i] - before.max[i]).abs() < 1e-3);
    }
    if !k.is_truck() {
        assert_eq!(k.faces(back).unwrap().len(), k.faces(h).unwrap().len());
    }
}

/// 10-20 p7: 高さが 0.02 mm だけ違う箱を並べて足した形(段の所に幅 0.02 mm の細い面ができる)の
/// 表示と STL の網は、閉じている。頂点を位置で寄せると、どの辺もちょうど 2 つの三角形に使われる
/// (穴が空けば 1 つだけの辺ができる)。
#[test]
fn a_mesh_of_a_shape_with_a_thin_face_has_no_gaps() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!(
            "truck は同じ平面で接する形のブーリアンに失敗する(隣り合う 2 枚の板の側面と下の面)"
        );
    }
    let mut b = DocBuilder::new();
    let a = plate(&mut b, [0.0, 0.0, 20.0, 20.0], 10.0);
    let s = rect_sketch(&mut b, SketchPlane::Xy, 20.0, 0.0, 20.0, 20.0);
    b.feature(
        EXTRUDE,
        "少し高い板",
        Extrude {
            profile: Some(s),
            distance: "10.02 mm".into(),
            operation: Operation::Join,
            target: Some(a),
            ..Default::default()
        },
    );
    let mut doc = b.finish("細い面");
    let h = body(&mut doc, &k);
    let thin = k
        .faces(h)
        .unwrap()
        .into_iter()
        .filter(|f| f.area < 1.0)
        .count();
    assert!(thin >= 1, "細い面がある");
    for tol in [0.1, 0.01] {
        let m = k.mesh(h, tol).unwrap();
        // 位置を 1e-4 mm で丸めて同じ頂点にする
        let key = |v: u32| m.positions[v as usize].map(|c| (f64::from(c) * 1e4).round() as i64);
        let mut edges: std::collections::HashMap<([i64; 3], [i64; 3]), usize> = Default::default();
        for t in &m.triangles {
            for i in 0..3 {
                let (p, q) = (key(t[i]), key(t[(i + 1) % 3]));
                if p == q {
                    continue;
                }
                *edges
                    .entry(if p < q { (p, q) } else { (q, p) })
                    .or_default() += 1;
            }
        }
        let open: Vec<_> = edges.iter().filter(|(_, n)| **n != 2).collect();
        assert!(
            open.is_empty(),
            "{tol}: 閉じていない辺 {} 本 {:?}",
            open.len(),
            &open[..open.len().min(4)]
        );
    }
}

/// 読み込む STEP: 20×20×10 の箱(JC で書いた物)。
fn box_step(k: &TestKernel) -> String {
    let mut b = DocBuilder::new();
    plate(&mut b, [0.0, 0.0, 20.0, 20.0], 10.0);
    let h = body(&mut b.finish("元"), k);
    k.export_step(&[h]).unwrap()
}

fn import(b: &mut DocBuilder, step: &str) -> kmb_core::FeatureId {
    b.feature(
        IMPORT_STEP,
        "読み込み",
        ImportStep {
            file: "元.step".into(),
            step: step.into(),
            stand_up: false,
            upright: false,
        },
    )
}

/// 10-22 i5: 読み込んだ STEP の部品の上の面にスケッチを描いて結合で押し出すと、読み込んだ部品に
/// 足され、ボディは 1 つのまま体積が増える(別の形にならない)。
#[test]
fn an_extrude_joined_from_an_imported_face_adds_to_that_part() {
    let k = TestKernel::new();
    if k.is_truck() {
        // truck は STEP を網で読むので、その面から押し出せない
        kmb_contract::skip!("truck は読み込んだ STEP を網で読む");
    }
    let step = box_step(&k);
    // 上の面の名前を、読み込んだだけの文書で探す
    let mut b = DocBuilder::new();
    import(&mut b, &step);
    let h = body(&mut b.finish("読み込み"), &k);
    let top = k
        .faces(h)
        .unwrap()
        .into_iter()
        .find(|f| f.planar.is_some_and(|p| p.normal[2] > 0.99))
        .expect("上の面")
        .name;
    let mut b = DocBuilder::new();
    let part = import(&mut b, &step);
    let mut s = SketchBuilder::new(SketchPlane::face(part, top));
    let o = s.fixed_point(0.0, 0.0);
    let c = s.point_from(o, "5 mm", "5 mm", [5.0, 5.0]);
    s.rectangle(c, "10 mm", "10 mm", [5.0, 5.0, 10.0, 10.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "上の輪郭", s.build());
    b.feature(
        EXTRUDE,
        "足す",
        Extrude {
            profile: Some(sk),
            distance: "5 mm".into(),
            operation: Operation::Join,
            target: Some(part),
            ..Default::default()
        },
    );
    let m = final_mass(&mut b.finish("読み込みに足す"), &k);
    assert!(
        close(m.volume, 20.0 * 20.0 * 10.0 + 10.0 * 10.0 * 5.0, 1e-6),
        "{}",
        m.volume
    );
    assert!(close(m.max[2], 15.0, 1e-6), "{m:?}");
}
