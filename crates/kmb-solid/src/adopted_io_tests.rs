//! ねじ穴のある部品の STEP の書き出し、細い面のある形の表示と STL、入れ子の STEP の面からの押し出し。

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

/// ねじ穴(M6 のめねじ、貫通)を 2 つ開けた板を STEP に書き出すと止まらずに書け、
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

/// 高さが 0.02 mm だけ違う箱を並べて足した形(段の所に幅 0.02 mm の細い面ができる)の
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
        assert_closed_triangles(
            &m.triangles
                .iter()
                .map(|t| t.map(|i| m.positions[i as usize]))
                .collect::<Vec<_>>(),
            &format!("表示 {tol}"),
        );
        let mut stl = Vec::new();
        kmb_io_stl::write_binary(&[m], &mut stl).unwrap();
        let count = u32::from_le_bytes(stl[80..84].try_into().unwrap()) as usize;
        assert_eq!(stl.len(), 84 + 50 * count);
        let triangles: Vec<[[f32; 3]; 3]> = stl[84..]
            .as_chunks::<50>()
            .0
            .iter()
            .map(|facet| {
                std::array::from_fn(|i| {
                    std::array::from_fn(|j| {
                        let at = 12 + i * 12 + j * 4;
                        f32::from_le_bytes(facet[at..at + 4].try_into().unwrap())
                    })
                })
            })
            .collect();
        assert_eq!(triangles.len(), count);
        assert_closed_triangles(&triangles, &format!("STL {tol}"));
    }
}

/// 同じ位置の頂点を 1e-4 mm で寄せ、すべての辺が三角形 2 枚に使われることを確かめる。
fn assert_closed_triangles(triangles: &[[[f32; 3]; 3]], what: &str) {
    assert!(!triangles.is_empty(), "{what}: 三角形がある");
    let key = |p: [f32; 3]| p.map(|c| (f64::from(c) * 1e4).round() as i64);
    let mut edges: std::collections::HashMap<([i64; 3], [i64; 3]), usize> = Default::default();
    for t in triangles {
        for i in 0..3 {
            let (p, q) = (key(t[i]), key(t[(i + 1) % 3]));
            if p != q {
                *edges
                    .entry(if p < q { (p, q) } else { (q, p) })
                    .or_default() += 1;
            }
        }
    }
    let open: Vec<_> = edges.iter().filter(|(_, n)| **n != 2).collect();
    assert!(
        open.is_empty(),
        "{what}: 閉じていない辺 {} 本 {:?}",
        open.len(),
        &open[..open.len().min(4)]
    );
}

/// 読み込む STEP: 20×20×10 の箱(JC で書いた物)。
fn box_step(k: &TestKernel) -> String {
    let mut b = DocBuilder::new();
    plate(&mut b, [0.0, 0.0, 20.0, 20.0], 10.0);
    let h = body(&mut b.finish("元"), k);
    k.export_step(&[h]).unwrap()
}

/// 自分で書いた箱を、外箱→内箱→箱の二段の部品関係に入れる。
/// 外箱から内箱は X に 30 mm、内箱から箱は Y に 20 mm ずらす。
fn nested_box_step(k: &TestKernel) -> String {
    let step = box_step(k);
    let records: Vec<(u64, &str)> = step
        .split(';')
        .filter_map(|s| {
            let s = s.get(s.find('#')? + 1..)?;
            let (id, rhs) = s.split_once('=')?;
            Some((id.trim().parse().ok()?, rhs.trim()))
        })
        .collect();
    let refs = |s: &str| -> Vec<u64> {
        s.split('#')
            .skip(1)
            .map(|s| {
                s.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap()
            })
            .collect()
    };
    let find = |name: &str| {
        records
            .iter()
            .find(|(_, r)| r.starts_with(name))
            .unwrap_or_else(|| panic!("STEP に {name} がある"))
    };
    let (leaf, _) = find("PRODUCT_DEFINITION(");
    let (_, shape_def) = find("SHAPE_DEFINITION_REPRESENTATION(");
    let leaf_shape = refs(shape_def)[1];
    let (_, brep) = find("ADVANCED_BREP_SHAPE_REPRESENTATION(");
    let geom = *refs(brep).last().unwrap();
    let context = find("PRODUCT_DEFINITION_CONTEXT(").0;
    let product_context = find("PRODUCT_CONTEXT(").0;
    let n = records.iter().map(|(i, _)| *i).max().unwrap() + 1;
    let id = |i: u64| n + i;
    let extra = format!(
        r#"
#{p0}=PRODUCT('outer','outer','',(#{pc}));
#{f0}=PRODUCT_DEFINITION_FORMATION('','',#{p0});
#{d0}=PRODUCT_DEFINITION('outer','',#{f0},#{ctx});
#{s0}=PRODUCT_DEFINITION_SHAPE('','',#{d0});
#{r0}=SHAPE_REPRESENTATION('',(#{axis0}),#{geom});
#{sd0}=SHAPE_DEFINITION_REPRESENTATION(#{s0},#{r0});
#{p1}=PRODUCT('inner','inner','',(#{pc}));
#{f1}=PRODUCT_DEFINITION_FORMATION('','',#{p1});
#{d1}=PRODUCT_DEFINITION('inner','',#{f1},#{ctx});
#{s1}=PRODUCT_DEFINITION_SHAPE('','',#{d1});
#{r1}=SHAPE_REPRESENTATION('',(#{axis0}),#{geom});
#{sd1}=SHAPE_DEFINITION_REPRESENTATION(#{s1},#{r1});
#{point0}=CARTESIAN_POINT('',(0.,0.,0.));
#{point1}=CARTESIAN_POINT('',(30.,0.,0.));
#{point2}=CARTESIAN_POINT('',(0.,20.,0.));
#{axis0}=AXIS2_PLACEMENT_3D('',#{point0},$,$);
#{axis1}=AXIS2_PLACEMENT_3D('',#{point1},$,$);
#{axis2}=AXIS2_PLACEMENT_3D('',#{point2},$,$);
#{use0}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('outer-inner','','',#{d0},#{d1},$);
#{us0}=PRODUCT_DEFINITION_SHAPE('','',#{use0});
#{tr0}=ITEM_DEFINED_TRANSFORMATION('','',#{axis0},#{axis1});
#{rr0}=(REPRESENTATION_RELATIONSHIP('','',#{r1},#{r0}) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{tr0}) SHAPE_REPRESENTATION_RELATIONSHIP());
#{cd0}=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{rr0},#{us0});
#{use1}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('inner-leaf','','',#{d1},#{leaf},$);
#{us1}=PRODUCT_DEFINITION_SHAPE('','',#{use1});
#{tr1}=ITEM_DEFINED_TRANSFORMATION('','',#{axis0},#{axis2});
#{rr1}=(REPRESENTATION_RELATIONSHIP('','',#{leaf_shape},#{r1}) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{tr1}) SHAPE_REPRESENTATION_RELATIONSHIP());
#{cd1}=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{rr1},#{us1});
"#,
        p0 = id(0),
        f0 = id(1),
        d0 = id(2),
        s0 = id(3),
        r0 = id(4),
        sd0 = id(5),
        p1 = id(6),
        f1 = id(7),
        d1 = id(8),
        s1 = id(9),
        r1 = id(10),
        sd1 = id(11),
        point0 = id(12),
        point1 = id(13),
        point2 = id(14),
        axis0 = id(15),
        axis1 = id(16),
        axis2 = id(17),
        use0 = id(18),
        us0 = id(19),
        tr0 = id(20),
        rr0 = id(21),
        cd0 = id(22),
        use1 = id(23),
        us1 = id(24),
        tr1 = id(25),
        rr1 = id(26),
        cd1 = id(27),
        pc = product_context,
        ctx = context,
    );
    assert_eq!(extra.matches("NEXT_ASSEMBLY_USAGE_OCCURRENCE").count(), 2);
    let end = step.rfind("ENDSEC;").unwrap();
    format!("{}{}{}", &step[..end], extra, &step[end..])
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

/// 二段に入れ子になった STEP の部品の上の面にスケッチを描いて結合で押し出すと、読み込んだ部品に
/// 足され、ボディは 1 つのまま体積が増える(別の形にならない)。
#[test]
fn an_extrude_joined_from_an_imported_face_adds_to_that_part() {
    let k = TestKernel::new();
    if k.is_truck() {
        // truck は STEP を網で読むので、その面から押し出せない
        kmb_contract::skip!("truck は読み込んだ STEP を網で読む");
    }
    let step = nested_box_step(&k);
    // 上の面の名前を、読み込んだだけの文書で探す
    let mut b = DocBuilder::new();
    import(&mut b, &step);
    let h = body(&mut b.finish("読み込み"), &k);
    eprintln!("入れ子の配置: {:?}", k.mass(h).unwrap());
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
