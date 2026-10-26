//! 平らな面を押し出す(選択と複数設定の UI の分析の R15)。

use super::*;
use kmb_core::{Document, FeatureId, FeatureStatus, Kernel, Ref, Registry, Value};
use kmb_kernel_smlib::SmlibKernel;
use kmb_samples::sample::{DocBuilder, SketchBuilder, reference_part};
use kmb_sketch::SketchPlane;

fn registry() -> Registry {
    let mut r = Registry::new();
    kmb_sketch::register(&mut r);
    register(&mut r);
    r
}

/// 板: 30 × 20 の長方形を 5 mm 押し出す。`hole` があれば、真ん中に直径 `hole` の穴の輪郭も描く。
fn plate(hole: Option<f64>) -> (DocBuilder, FeatureId) {
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "30 mm", "20 mm", [0.0, 0.0, 30.0, 20.0]);
    if let Some(d) = hole {
        let c = s.point(15.0, 10.0);
        s.circle(c, &format!("{d} mm"), d / 2.0);
    }
    let sk = b.feature(kmb_sketch::SKETCH, "板の形", s.build());
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(sk),
            distance: "5 mm".into(),
            ..Default::default()
        },
    );
    (b, plate)
}

fn face(feature: FeatureId, name: &str) -> Ref {
    Ref::Face {
        feature,
        face: name.into(),
    }
}

/// 面 `faces` を `distance` だけ押し出して、操作 `op` で板に当てる。
fn face_extrude(
    b: &mut DocBuilder,
    target: FeatureId,
    faces: Vec<Ref>,
    distance: &str,
    direction: Direction,
    op: Operation,
) -> FeatureId {
    b.feature(
        EXTRUDE,
        "面の押し出し",
        Extrude {
            source: Source::Faces,
            faces,
            distance: distance.into(),
            direction,
            operation: op,
            target: Some(target),
            ..Default::default()
        },
    )
}

/// 最後のボディの体積(どの機能も計算できていること)。
fn volume(doc: &mut Document, k: &dyn Kernel) -> f64 {
    let reg = registry();
    let report = doc.recompute(&reg, k);
    for f in &report.features {
        assert!(
            matches!(f.status, FeatureStatus::Ok { .. }),
            "{}: {:?}",
            f.name,
            f.status
        );
    }
    let bodies = visible_bodies(doc.data(), &reg, &report);
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    k.mass(bodies[0].1).unwrap().volume
}

fn status_of(doc: &mut Document, k: &dyn Kernel, id: FeatureId) -> FeatureStatus {
    let reg = registry();
    let report = doc.recompute(&reg, k);
    report.feature(id).unwrap().clone()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-3 * b.abs().max(1.0)
}

/// 上面の押し出しを、板の文書 1 つで押し出しのデータだけ替えて確かめる(穴のある板は別の文書)。
/// - 上へ押し出すと板に足す
/// - 逆向きに体へ押し込むと切り取る
/// - 穴のある面は穴を残して押し出す
#[test]
fn the_top_face_extrudes_up_or_cuts_down_and_keeps_its_hole() {
    let k = SmlibKernel::new();
    let up = |p: FeatureId, distance: &str, direction, op| Extrude {
        source: Source::Faces,
        faces: vec![face(p, &format!("f{}:end", p.0))],
        distance: distance.into(),
        direction,
        operation: op,
        target: Some(p),
        ..Default::default()
    };
    let (mut b, p) = plate(None);
    let id = b.feature(
        EXTRUDE,
        "面の押し出し",
        up(p, "4 mm", Direction::Normal, Operation::Join),
    );
    let mut doc = b.finish("面の押し出し");
    for (distance, direction, op, want) in [
        (
            "4 mm",
            Direction::Normal,
            Operation::Join,
            30.0 * 20.0 * 9.0,
        ),
        (
            "2 mm",
            Direction::Reverse,
            Operation::Cut,
            30.0 * 20.0 * 3.0,
        ),
    ] {
        let data = serde_json::to_value(up(p, distance, direction, op)).unwrap();
        doc.apply(kmb_core::Transaction::new(
            "面の押し出し",
            vec![kmb_core::Op::SetFeatureData { id, data }],
        ))
        .unwrap();
        let v = volume(&mut doc, &k);
        assert!(close(v, want), "{distance} {op:?}: {v} vs {want}");
    }

    // 穴のある面
    let (mut b, p) = plate(Some(8.0));
    b.feature(
        EXTRUDE,
        "面の押し出し",
        up(p, "3 mm", Direction::Normal, Operation::Join),
    );
    let mut doc = b.finish("穴のある面");
    let v = volume(&mut doc, &k);
    let area = 30.0 * 20.0 - std::f64::consts::PI * 16.0;
    assert!(close(v, area * 8.0), "{v} vs {}", area * 8.0);
}

#[test]
fn a_side_face_extrudes_sideways() {
    let k = SmlibKernel::new();
    let reg = registry();
    let (b, p) = plate(None);
    let mut doc = b.finish("横");
    let report = doc.recompute(&reg, &k);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(p) else {
        panic!("板がない")
    };
    let Some(Value::Shape(h)) = outputs.get("body") else {
        panic!("板がない")
    };
    // 平らな側面のどれか 1 つ(面積の分だけ体積が増える)
    let side = k
        .faces(*h)
        .unwrap()
        .into_iter()
        .find(|f| f.name.contains(":side") && f.planar.is_some())
        .unwrap();
    let (mut b, p) = plate(None);
    face_extrude(
        &mut b,
        p,
        vec![face(p, &side.name)],
        "6 mm",
        Direction::Normal,
        Operation::Join,
    );
    let mut doc = b.finish("横");
    let v = volume(&mut doc, &k);
    assert!(close(v, 3000.0 + side.area * 6.0), "{v} vs {}", side.area);
}

#[test]
fn the_reference_part_top_face_with_its_pocket_opening_extrudes() {
    // 参考の部品の平らな面のうち、いちばん高い所の面(ポケットの口が穴になる)を押し出す
    let k = SmlibKernel::new();
    let reg = registry();
    let mut part = reference_part();
    let report = part.doc.recompute(&reg, &k);
    let bodies = visible_bodies(part.doc.data(), &reg, &report);
    let (last, h) = bodies[0];
    let before = k.mass(h).unwrap().volume;
    let top = k
        .faces(h)
        .unwrap()
        .into_iter()
        .filter(|f| f.planar.is_some_and(|p| p.normal[2] > 0.99))
        .max_by(|a, b| {
            let z = |f: &kmb_core::FaceInfo| f.planar.unwrap().point[2];
            z(a).total_cmp(&z(b))
        })
        .unwrap();
    let id = part.doc.alloc_feature_id();
    part.doc
        .apply(kmb_core::Transaction::new(
            "面の押し出し",
            vec![kmb_core::Op::InsertFeature {
                index: None,
                entry: kmb_core::FeatureEntry {
                    id,
                    type_id: EXTRUDE.into(),
                    name: "面の押し出し".into(),
                    suppressed: false,
                    data: serde_json::to_value(Extrude {
                        source: Source::Faces,
                        faces: vec![face(last, &top.name)],
                        distance: "2 mm".into(),
                        operation: Operation::Join,
                        target: Some(last),
                        ..Default::default()
                    })
                    .unwrap(),
                },
            }],
        ))
        .unwrap();
    let doc = &mut part.doc;
    let v = volume(doc, &k);
    assert!(
        close(v, before + top.area * 2.0),
        "{v} vs {before} + {}",
        top.area
    );
}

#[test]
fn two_coplanar_faces_extrude_together() {
    // 板の底に溝を切って、底の面を同じ平面の 2 枚に分け、2 枚を一緒に下へ押し出す
    let k = SmlibKernel::new();
    let reg = registry();
    let slot = |b: &mut DocBuilder, p: FeatureId| {
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        let o = s.fixed_point(0.0, 0.0);
        let c = s.point_from(o, "12 mm", "1 mm", [12.0, -1.0]);
        s.rectangle(c, "6 mm", "22 mm", [12.0, -1.0, 6.0, 22.0]);
        let sk = b.feature(kmb_sketch::SKETCH, "溝の形", s.build());
        b.feature(
            EXTRUDE,
            "溝",
            Extrude {
                profile: Some(sk),
                distance: "2 mm".into(),
                operation: Operation::Cut,
                target: Some(p),
                ..Default::default()
            },
        )
    };
    let (mut b, p) = plate(None);
    let cut = slot(&mut b, p);
    let mut doc = b.finish("溝");
    let report = doc.recompute(&reg, &k);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(cut) else {
        panic!("溝が切れない")
    };
    let Some(Value::Shape(h)) = outputs.get("body") else {
        panic!("溝が切れない")
    };
    let bottoms: Vec<String> = k
        .faces(*h)
        .unwrap()
        .into_iter()
        .filter(|f| {
            f.planar
                .is_some_and(|q| q.normal[2] < -0.99 && q.point[2].abs() < 1e-6)
        })
        .map(|f| f.name)
        .collect();
    assert_eq!(bottoms.len(), 2, "{bottoms:?}");
    let (mut b, p) = plate(None);
    let cut = slot(&mut b, p);
    face_extrude(
        &mut b,
        cut,
        bottoms.iter().map(|n| face(cut, n)).collect(),
        "3 mm",
        Direction::Normal,
        Operation::Join,
    );
    let mut doc = b.finish("2 枚");
    let v = volume(&mut doc, &k);
    let want = 30.0 * 20.0 * 5.0 - 6.0 * 20.0 * 2.0 + 2.0 * 12.0 * 20.0 * 3.0;
    assert!(close(v, want), "{v} vs {want}");
}

/// 板の面のうち、外向きの法線が `n` の平らな面の名前。
fn face_toward(k: &dyn Kernel, doc: &mut Document, p: FeatureId, n: [f64; 3]) -> String {
    let report = doc.recompute(&registry(), k);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(p) else {
        panic!("板が作れない")
    };
    let Some(Value::Shape(h)) = outputs.get("body") else {
        panic!("板が作れない")
    };
    k.faces(*h)
        .unwrap()
        .into_iter()
        .find(|f| {
            f.planar
                .is_some_and(|q| (0..3).all(|i| (q.normal[i] - n[i]).abs() < 1e-6))
        })
        .expect("その向きの面がある")
        .name
}

#[test]
fn faces_on_different_planes_extrude_along_their_own_normals() {
    // 上・下・横(x の大きい側)の面を一緒に選ぶと、それぞれの面の外向きへ押し出す。
    // 上と横の道具は板の角の辺で接するだけ
    let k = SmlibKernel::new();
    let (b, p) = plate(None);
    let mut doc = b.finish("板");
    let side = face_toward(&k, &mut doc, p, [1.0, 0.0, 0.0]);
    let top = format!("f{}:end", p.0);
    let bottom = format!("f{}:start", p.0);
    let plate_v = 30.0 * 20.0 * 5.0;
    for (faces, dir, op, want) in [
        (
            vec![&top, &bottom, &side],
            Direction::Normal,
            Operation::Join,
            plate_v + 2.0 * 30.0 * 20.0 + 20.0 * 5.0,
        ),
        // 逆向きに押し込むと面ごとに切り取る(上と横の切り取りは角で重なる)
        (
            vec![&top, &side],
            Direction::Reverse,
            Operation::Cut,
            plate_v - 30.0 * 20.0 - 20.0 * 5.0 + 20.0,
        ),
    ] {
        let (mut b, p) = plate(None);
        let id = face_extrude(
            &mut b,
            p,
            faces.iter().map(|n| face(p, n)).collect(),
            "1 mm",
            dir,
            op,
        );
        let mut doc = b.finish("平面の違う面");
        let v = volume(&mut doc, &k);
        assert!(close(v, want), "{op:?}: {v} vs {want}");
        // 下絵は面ごとの輪郭を出す
        let report = doc.recompute(&registry(), &k);
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
            panic!("押し出せない")
        };
        let preview: kmb_core::ToolPreview =
            kmb_core::Payload::from_value(&outputs[kmb_core::TOOL_PREVIEW]).unwrap();
        assert_eq!(preview.profile.len(), faces.len());
    }
}

#[test]
fn bad_picks_are_explained() {
    let k = SmlibKernel::new();
    // 面でない物(ボディ)は押し出せない、何も選ばなければそう言う(板の機能の番号はどの文書も同じ)
    let p = plate(None).1;
    for (faces, word) in [
        (vec![Ref::Body(p)], "平らな面だけ"),
        (vec![], "平らな面を選んで"),
    ] {
        let (mut b, p) = plate(None);
        let id = face_extrude(&mut b, p, faces, "1 mm", Direction::Normal, Operation::Join);
        let mut doc = b.finish("誤り");
        match status_of(&mut doc, &k, id) {
            FeatureStatus::Error { message } => {
                assert!(message.contains(word), "{message}")
            }
            s => panic!("{s:?}"),
        }
    }
}

#[test]
fn the_face_extrude_data_reads_and_old_sketch_extrudes_stay_the_same() {
    // 前の形のデータ(スケッチの番号だけ)はそのまま読めて、書き戻しても同じ
    let old = serde_json::json!({"profile": 1, "distance": "5 mm", "operation": "new"});
    let e: Extrude = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(e.profile, Some(FeatureId(1)));
    assert_eq!(e.source, Source::Sketch);
    let back = serde_json::to_value(&e).unwrap();
    assert!(
        back.get("source").is_none() && back.get("faces").is_none(),
        "{back}"
    );
    // 面の押し出しは面 1 つなら値そのもので書く
    let e = Extrude {
        source: Source::Faces,
        faces: vec![face(FeatureId(2), "f2:end")],
        target: Some(FeatureId(2)),
        operation: Operation::Join,
        ..Default::default()
    };
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(v["source"], "faces");
    assert_eq!(v["faces"]["face"], "f2:end");
    assert!(v.get("profile").is_none(), "{v}");
    assert_eq!(serde_json::from_value::<Extrude>(v).unwrap(), e);
}

#[test]
fn picking_a_flat_face_then_extrude_makes_a_face_extrude_that_joins() {
    use kmb_core::{Context, Extension, FacePick, Outcome};
    let k: std::sync::Arc<dyn Kernel> = std::sync::Arc::new(SmlibKernel::new());
    let reg = registry();
    let (b, p) = plate(None);
    let mut doc = b.finish("板");
    let report = doc.recompute(&reg, k.as_ref());
    let top = FacePick {
        feature: p,
        face: format!("f{}:end", p.0),
    };
    let args = serde_json::json!({"distance": 4.0});
    let cx = Context {
        picked: Some(&top),
        args: &args,
        ..Context::new(doc.data(), &report, &reg, &k)
    };
    // 高さはその面から(手順は高さだけ。押すと結合、引くと切り取りの色)
    let h = ExtrudeCommands.handles(EXTRUDE_SKETCH, &cx).unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].key, "distance");
    let Some(Ok(Outcome::Change { edits, .. })) = ExtrudeCommands.run(EXTRUDE_SKETCH, &cx) else {
        panic!("押し出しを作れない")
    };
    let kmb_core::Edit::Insert { data, .. } = &edits[0] else {
        panic!("{edits:?}")
    };
    let e: Extrude = serde_json::from_value(data.clone()).unwrap();
    assert_eq!(e.source, Source::Faces);
    assert_eq!(e.faces, vec![face(p, &top.face)]);
    assert_eq!((e.operation, e.target), (Operation::Join, Some(p)));
    assert_eq!(e.distance, "4 mm");

    // 曲がった面(穴の内側)を選んでいる時は、面の押し出しにしない(スケッチの押し出しのまま)
    let (b, p) = plate(Some(8.0));
    let mut doc = b.finish("穴");
    let report = doc.recompute(&reg, k.as_ref());
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(p) else {
        panic!()
    };
    let Some(Value::Shape(h)) = outputs.get("body") else {
        panic!()
    };
    let curved = k
        .faces(*h)
        .unwrap()
        .into_iter()
        .find(|f| f.planar.is_none())
        .unwrap();
    let pick = FacePick {
        feature: p,
        face: curved.name,
    };
    let cx = Context {
        picked: Some(&pick),
        ..Context::new(doc.data(), &report, &reg, &k)
    };
    if let Some(Ok(Outcome::Change { edits, .. })) = ExtrudeCommands.run(EXTRUDE_SKETCH, &cx) {
        let kmb_core::Edit::Insert { data, .. } = &edits[0] else {
            panic!("{edits:?}")
        };
        assert!(data.get("source").is_none(), "{data}");
    }
}

/// 箱(x 0〜40、y 0〜20、高さ 30)の上の x = 40 の辺を 20 mm 面取りして、斜めの面
/// (z = 50 − x、45°)を作る。`ys` の y ごとに、y〜y + 10 の所に x 25〜35 の長方形を描く。
/// 返すのは (文書, 面取りの機能, 長方形のスケッチ(`ys` の順), 斜めの面の名前)。
/// 箱を 1 回計算して辺の名前を見つけ、その文書に面取りを足す(箱は作り直さない)。
fn wedge_and_posts(k: &SmlibKernel, ys: &[f64]) -> (Document, FeatureId, Vec<FeatureId>, String) {
    let reg = registry();
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "40 mm", "20 mm", [0.0, 0.0, 40.0, 20.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "箱の形", s.build());
    let block = b.feature(
        EXTRUDE,
        "箱",
        Extrude {
            profile: Some(sk),
            distance: "30 mm".into(),
            ..Default::default()
        },
    );
    let posts = ys
        .iter()
        .map(|&y| {
            let mut s = SketchBuilder::new(SketchPlane::Xy);
            let o = s.fixed_point(0.0, 0.0);
            let c = s.point_from(o, "25 mm", &format!("{y} mm"), [25.0, y]);
            s.rectangle(c, "10 mm", "10 mm", [25.0, y, 10.0, 10.0]);
            b.feature(kmb_sketch::SKETCH, "柱の形", s.build())
        })
        .collect();
    let mut doc = b.finish("斜めの面");
    // 上の面と x = 40 の横の面の間の辺
    let report = doc.recompute(&reg, k);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(block) else {
        panic!()
    };
    let Some(Value::Shape(h)) = outputs.get("body") else {
        panic!()
    };
    let faces = k.faces(*h).unwrap();
    let side = faces
        .iter()
        .find(|f| f.planar.is_some_and(|p| p.normal[0] > 0.99))
        .unwrap()
        .name
        .clone();
    let top = format!("f{}:end", block.0);
    let ch = doc.alloc_feature_id();
    doc.apply(kmb_core::Transaction::new(
        "面取り",
        vec![kmb_core::Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id: ch,
                type_id: CHAMFER.into(),
                name: "面取り".into(),
                suppressed: false,
                data: serde_json::json!({"target": block.0, "edges": [[top, side]], "distance": "20 mm"}),
            },
        }],
    ))
    .unwrap();
    let report = doc.recompute(&reg, k);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(ch) else {
        panic!("{:?}", report.feature(ch))
    };
    let Some(Value::Shape(h)) = outputs.get("body") else {
        panic!()
    };
    let slant = k
        .faces(*h)
        .unwrap()
        .into_iter()
        .find(|f| {
            f.planar
                .is_some_and(|p| p.normal[0] > 0.5 && p.normal[2] > 0.5)
        })
        .expect("斜めの面")
        .name;
    (doc, ch, posts, slant)
}

/// 斜めの面までの押し出しを、面取りした箱の文書 1 つで、最後の押し出しだけ替えて確かめる。
/// - 箱の外の柱は、端が斜めの面に沿って切れる(ずらし −5 なら 5 mm 下)
/// - 箱の中の長方形を底から斜めの面まで切り取ると、その下の 2000 mm³ だけ消える
#[test]
fn extrude_to_a_slanted_face_ends_or_cuts_along_that_face() {
    let k = SmlibKernel::new();
    let reg = registry();
    let (mut doc, ch, posts, slant) = wedge_and_posts(&k, &[30.0, 5.0]);
    let (outside, inside) = (posts[0], posts[1]);
    let to_slant = |profile: FeatureId, offset: Option<&str>| Extrude {
        profile: Some(profile),
        distance: "1 mm".into(),
        extent: kmb_cad::Extent::ToFace,
        to_face: Some(ToFace {
            feature: ch,
            face: slant.clone(),
            vertex: None,
        }),
        to_offset: offset.map(String::from),
        ..Default::default()
    };
    let id = doc.alloc_feature_id();
    doc.apply(kmb_core::Transaction::new(
        "斜めの面まで",
        vec![kmb_core::Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: EXTRUDE.into(),
                name: "柱".into(),
                suppressed: false,
                data: serde_json::to_value(to_slant(outside, None)).unwrap(),
            },
        }],
    ))
    .unwrap();
    let set = |doc: &mut Document, data: Extrude| {
        let data = serde_json::to_value(data).unwrap();
        doc.apply(kmb_core::Transaction::new(
            "斜めの面まで",
            vec![kmb_core::Op::SetFeatureData { id, data }],
        ))
        .unwrap();
    };
    for (offset, want) in [(None, 2000.0), (Some("-5 mm"), 1500.0)] {
        set(&mut doc, to_slant(outside, offset));
        let report = doc.recompute(&reg, &k);
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
            panic!("{:?}", report.feature(id))
        };
        let Some(Value::Shape(h)) = outputs.get("body") else {
            panic!()
        };
        // 柱の上は z = 50 − x(x 25〜35 で平均 20)、ずらし −5 なら 5 mm 下
        let m = k.mass(*h).unwrap();
        assert!(close(m.volume, want), "{offset:?}: {}", m.volume);
        assert!((m.max[2] - (25.0 - 5.0 * offset.is_some() as u8 as f64)).abs() < 1e-3);
    }

    // 箱の中の長方形を、底から斜めの面まで切り取る(下の 2000 mm³ だけ消える)
    set(
        &mut doc,
        Extrude {
            operation: Operation::Cut,
            target: Some(ch),
            ..to_slant(inside, None)
        },
    );
    let v = volume(&mut doc, &k);
    // 箱 24000 − 面取り 4000 − 切り取り 2000
    assert!(close(v, 18000.0), "{v}");
}

/// 文書の末尾に機能を足す。
fn push(doc: &mut Document, type_id: &str, data: serde_json::Value) -> FeatureId {
    let id = doc.alloc_feature_id();
    doc.apply(kmb_core::Transaction::new(
        type_id,
        vec![kmb_core::Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: type_id.into(),
                name: type_id.into(),
                suppressed: false,
                data,
            },
        }],
    ))
    .unwrap();
    id
}

/// 機能 `id` の出力のボディ。
fn body_of(doc: &mut Document, k: &dyn Kernel, id: FeatureId) -> kmb_core::ShapeHandle {
    let report = doc.recompute(&registry(), k);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
        panic!("{:?}", report.feature(id))
    };
    let Some(Value::Shape(h)) = outputs.get("body") else {
        panic!()
    };
    *h
}

/// 面の押し出しを `face` から作り、足した・削った体積と、計算した形が測れること(壊れていないこと)を返す。
fn face_extrude_change(
    doc: &mut Document,
    k: &dyn Kernel,
    target: FeatureId,
    face_name: &str,
    distance: &str,
    direction: Direction,
    op: Operation,
) -> Result<f64, String> {
    let base = k.mass(body_of(doc, k, target)).unwrap().volume;
    let id = push(
        doc,
        EXTRUDE,
        serde_json::to_value(Extrude {
            source: Source::Faces,
            faces: vec![face(target, face_name)],
            distance: distance.into(),
            direction,
            operation: op,
            target: Some(target),
            ..Default::default()
        })
        .unwrap(),
    );
    let report = doc.recompute(&registry(), k);
    let out = match report.feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => match outputs.get("body") {
            Some(Value::Shape(h)) => k
                .mass(*h)
                .map(|m| m.volume - base)
                .map_err(|e| format!("壊れた形: {e}")),
            _ => Err("ボディが無い".into()),
        },
        other => Err(format!("{other:?}")),
    };
    doc.apply(kmb_core::Transaction::new(
        "戻す",
        vec![kmb_core::Op::RemoveFeature { id }],
    ))
    .unwrap();
    out
}

/// フィレットで丸めた面全体から押し出す。縦の 4 辺を半径 4 mm で丸めた箱の
/// 上の辺を面取りすると、斜めの面と丸めの境が楕円になり、面の縁を直線と円弧にできない。縁を
/// 網の折れ線で近い形にした輪郭を押し出すと、壁がもとの辺からわずかにずれて、結合も切り取りも
/// 「計算できた」まま測れない壊れた形になる。なので面の縁の曲線のまま押し出す。
/// 縁が直線と円弧の上の面は、これまでどおり輪郭で押し出す(面の名前を保つため)
#[test]
fn a_face_rounded_by_fillets_extrudes_from_its_own_edges() {
    let k = SmlibKernel::new();
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "40 mm", "20 mm", [0.0, 0.0, 40.0, 20.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "箱の形", s.build());
    let block = b.feature(
        EXTRUDE,
        "箱",
        Extrude {
            profile: Some(sk),
            distance: "30 mm".into(),
            ..Default::default()
        },
    );
    let mut doc = b.finish("丸めた面");
    let h = body_of(&mut doc, &k, block);
    let vertical: Vec<serde_json::Value> = k
        .edges(h)
        .unwrap()
        .into_iter()
        .filter(|e| e.faces.iter().all(|f| f.contains(":side:")))
        .map(|e| serde_json::json!({"kind": "edge", "feature": 0, "faces": e.faces}))
        .collect();
    assert_eq!(vertical.len(), 4);
    let fil = push(
        &mut doc,
        FILLET,
        serde_json::json!({"target": block.0, "rows": [{"edges": vertical, "radius": "4 mm"}]}),
    );
    let h = body_of(&mut doc, &k, fil);
    let side = k
        .faces(h)
        .unwrap()
        .into_iter()
        .find(|f| f.planar.is_some_and(|p| p.normal[0] > 0.99))
        .unwrap()
        .name;
    let top = format!("f{}:end", block.0);
    let ch = push(
        &mut doc,
        CHAMFER,
        serde_json::json!({"target": fil.0, "edges": [[top.clone(), side]], "distance": "20 mm"}),
    );
    let h = body_of(&mut doc, &k, ch);
    let infos = k.faces(h).unwrap();
    let slant = infos
        .iter()
        .find(|f| {
            f.planar
                .is_some_and(|p| p.normal[0] > 0.5 && p.normal[2] > 0.5)
        })
        .expect("斜めの面")
        .clone();
    let top_area = infos.iter().find(|f| f.name == top).unwrap().area;
    assert!(
        kmb_cad::face_profile(&k, h, std::slice::from_ref(&slant.name))
            .unwrap()
            .approximated,
        "斜めの面の縁は直線と円弧にできない(この試験の前提)"
    );
    for (name, area) in [(&slant.name, slant.area), (&top, top_area)] {
        let join = face_extrude_change(
            &mut doc,
            &k,
            ch,
            name,
            "5 mm",
            Direction::Normal,
            Operation::Join,
        )
        .unwrap_or_else(|e| panic!("{name} 結合: {e}"));
        assert!(
            close(join, 5.0 * area),
            "{name} 結合: {join}、{} のはず",
            5.0 * area
        );
        let cut = face_extrude_change(
            &mut doc,
            &k,
            ch,
            name,
            "3 mm",
            Direction::Reverse,
            Operation::Cut,
        )
        .unwrap_or_else(|e| panic!("{name} 切り取り: {e}"));
        assert!(
            close(-cut, 3.0 * area),
            "{name} 切り取り: {cut}、−{} のはず",
            3.0 * area
        );
    }
}

/// 上の 4 辺を丸めた板の上の面から押し出すと、壁は丸めの面と辺で接するだけになる。JC の SMLib では、結合も切り取りも面の面積 × 距離だけ変わる
#[test]
fn a_face_whose_walls_only_touch_its_rounds_extrudes_and_cuts() {
    let k = SmlibKernel::new();
    let (b, p) = plate(None);
    let mut doc = b.finish("板");
    let h = body_of(&mut doc, &k, p);
    let top = format!("f{}:end", p.0);
    let edges: Vec<serde_json::Value> = k
        .edges(h)
        .unwrap()
        .into_iter()
        .filter(|e| e.faces.contains(&top))
        .map(|e| serde_json::json!({"kind": "edge", "feature": 0, "faces": e.faces}))
        .collect();
    assert_eq!(edges.len(), 4);
    let fil = push(
        &mut doc,
        FILLET,
        serde_json::json!({"target": p.0, "rows": [{"edges": edges, "radius": "1 mm"}]}),
    );
    let h = body_of(&mut doc, &k, fil);
    let area = k
        .faces(h)
        .unwrap()
        .into_iter()
        .find(|f| f.name == top)
        .unwrap()
        .area;
    for (dir, op, sign) in [
        (Direction::Normal, Operation::Join, 1.0),
        (Direction::Reverse, Operation::Cut, -1.0),
    ] {
        let v = face_extrude_change(&mut doc, &k, fil, &top, "2 mm", dir, op)
            .unwrap_or_else(|e| panic!("{op:?}: {e}"));
        assert!(
            close(v, sign * 2.0 * area),
            "{op:?}: {v}、{} のはず",
            sign * 2.0 * area
        );
    }
}

/// 3D の見本のポケットの壁(面取りの角に幅 0.01 mm ほどの細い面がある)と、その細い面の縁を写した
/// 切り欠きのある箱を SMLib で結合すると、SMLib は成功と返して立体の無い形(体積 0、殻が裏返る)を
/// 出していた。カーネルが立体を確かめて失敗にする(docs/機能/ の「ブーリアンの結果が立体か確かめる」)。
#[test]
fn a_union_that_leaves_no_solid_is_refused_by_the_kernel() {
    use kmb_core::kernel::{Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment};
    let k = SmlibKernel::new();
    let reg = super::tests::registry();
    let mut doc = kmb_samples::reference_cam_3d_document();
    let cam = doc
        .data()
        .features
        .iter()
        .position(|f| f.type_id.starts_with("cam."));
    doc.apply(kmb_core::Transaction::new(
        "加工の手前まで",
        vec![kmb_core::Op::SetRollback { rollback: cam }],
    ))
    .unwrap();
    let report = doc.recompute(&reg, &k);
    let (_, body) = kmb_core::visible_bodies(doc.data(), &reg, &report)[0];
    // 壁 x = 8 の面の縁を 0.1 mm 延ばした輪郭(y, z)。y = 8 の側に、細い面を写した切り欠きがある
    let pts = [
        [7.9, 5.9],
        [20.1, 5.9],
        [20.1, 8.997],
        [8.014_142_135_623_73, 8.997],
        [8.014_142_135_623_734, 8.985_857_864_376_273],
        [8.003, 8.997],
        [7.9, 8.997],
    ];
    let edges = (0..pts.len())
        .map(|i| ProfileEdge {
            name: format!("e{i}"),
            segment: Segment::Line {
                a: pts[i],
                b: pts[(i + 1) % pts.len()],
            },
            reversed: false,
        })
        .collect();
    let profile = Profile {
        plane: Plane {
            origin: [8.0, 0.0, 0.0],
            x_axis: [0.0, 1.0, 0.0],
            y_axis: [0.0, 0.0, 1.0],
        },
        regions: vec![Region {
            outer: ProfileLoop { edges },
            holes: vec![],
        }],
    };
    let tool = k.extrude(&profile, -0.1, 10.0, "f99").unwrap();
    // 返すなら閉じた立体(SMLib が直って正しく結合できるようになっても通る)
    if let Ok(h) = k.boolean(kmb_core::BooleanOp::Union, body, tool) {
        let (_, solids, _) = k.check(h).unwrap();
        assert!(solids >= 1, "立体の無い形を返した(体積 {:?})", k.volume(h));
    }
    // 対象と道具を入れ替えると、SMLib は道具だけの正しい立体を返すことがある(上の箱を平らにした時)。
    // 体積を道具とも比べて止める
    let flat = Profile {
        regions: vec![Region {
            outer: ProfileLoop {
                edges: [[7.9, 5.9], [20.1, 5.9], [20.1, 8.995], [7.9, 8.995]]
                    .iter()
                    .enumerate()
                    .map(|(i, &a)| ProfileEdge {
                        name: format!("e{i}"),
                        segment: Segment::Line {
                            a,
                            b: [[20.1, 5.9], [20.1, 8.995], [7.9, 8.995], [7.9, 5.9]][i],
                        },
                        reversed: false,
                    })
                    .collect(),
            },
            holes: vec![],
        }],
        ..profile
    };
    let tool = k.extrude(&flat, -0.1, 10.0, "f99").unwrap();
    if let Ok(h) = kmb_cad::checked_boolean(&k, kmb_core::BooleanOp::Union, tool, body) {
        let v = k.volume(h).unwrap();
        assert!(v >= k.volume(body).unwrap(), "結合で対象が消えた(体積 {v})");
    }
}

/// 画面で同じ平面に無い平らな面(板の上と下)を選んで押し出しを押すと、面を黙って捨てて
/// スケッチの押し出しへ移らず、理由を出して止める(ボタンも同じ理由で押せない)。
/// 別のボディの面が混ざった時も同じ。曲がった面だけを選んでいる時は前のとおりスケッチの押し出し
#[test]
fn picking_flat_faces_of_two_bodies_stops_but_different_planes_extrude() {
    use kmb_core::{Context, Extension, FacePick};
    let k: std::sync::Arc<dyn Kernel> = std::sync::Arc::new(SmlibKernel::new());
    let reg = registry();
    let pick = |f: FeatureId, end: &str| FacePick {
        feature: f,
        face: format!("f{}:{end}", f.0),
    };
    // 板だけの文書(押し出せる輪郭は板の形の 1 つ。面を捨てると、これを押し出してしまう)と、
    // 板と別のボディ(柱)の文書
    let (b, p) = plate(None);
    let one = b.finish("板");
    let (mut b, _) = plate(None);
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(50.0, 0.0);
    s.rectangle(o, "10 mm", "10 mm", [50.0, 0.0, 60.0, 10.0]);
    let other = b.feature(kmb_sketch::SKETCH, "柱の形", s.build());
    let post = b.feature(
        EXTRUDE,
        "柱",
        Extrude {
            profile: Some(other),
            distance: "5 mm".into(),
            operation: Operation::New,
            ..Default::default()
        },
    );
    let two = b.finish("板と柱");
    // 同じボディの平面の違う面(上と下)は、両方を押し出す
    let mut doc = one;
    let report = doc.recompute(&reg, k.as_ref());
    let (top, also) = (pick(p, "end"), vec![pick(p, "start")]);
    let cx = Context {
        picked: Some(&top),
        also_picked: &also,
        ..Context::new(doc.data(), &report, &reg, &k)
    };
    match ExtrudeCommands.run(EXTRUDE_SKETCH, &cx) {
        Some(Ok(c)) => {
            let s = format!("{c:?}");
            assert!(s.contains("f2:start") && s.contains("f2:end"), "{s}")
        }
        other => panic!("上と下の面を押し出さない: {other:?}"),
    }
    let (mut doc, top, also, word) = (two, pick(p, "end"), vec![pick(post, "end")], "1 つのボディ");
    {
        let report = doc.recompute(&reg, k.as_ref());
        let cx = Context {
            picked: Some(&top),
            also_picked: &also,
            ..Context::new(doc.data(), &report, &reg, &k)
        };
        match ExtrudeCommands.run(EXTRUDE_SKETCH, &cx) {
            Some(Err(e)) => {
                let m = e.to_string();
                assert!(m.contains(word), "{m}")
            }
            other => panic!("面を捨てて押し出した: {other:?}"),
        }
        let a = ExtrudeCommands.actions(&cx);
        let why = a
            .iter()
            .find(|a| a.id == EXTRUDE_SKETCH)
            .and_then(|a| a.disabled.clone())
            .unwrap_or_default();
        assert!(why.contains(word), "{why}");
    }
}
