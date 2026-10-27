//! シェル(立体をくり抜いて殻にする)。

use super::tests::{TestKernel, close, final_mass, rect_sketch, registry};
use super::*;
use kmb_core::{FeatureId, FeatureStatus, Ref};
use kmb_samples::sample::{DocBuilder, reference_part};
use kmb_sketch::SketchPlane;

/// 20×20×10 の箱を作る。
fn block(b: &mut DocBuilder) -> FeatureId {
    let s = rect_sketch(b, SketchPlane::Xy, 0.0, 0.0, 20.0, 20.0);
    b.feature(
        EXTRUDE,
        "箱",
        Extrude {
            profile: Some(s),
            distance: "10 mm".into(),
            ..Default::default()
        },
    )
}

fn shell(faces: Vec<Ref>, direction: ShellDirection) -> Shell {
    Shell {
        faces,
        thickness: "2 mm".into(),
        direction,
        tangent_chain: false,
    }
}

fn top(block: FeatureId) -> Ref {
    Ref::Face {
        feature: block,
        face: format!("f{}:end", block.0),
    }
}

fn set(doc: &mut kmb_core::Document, id: FeatureId, data: Shell) {
    doc.apply(kmb_core::Transaction::new(
        "シェルを直す",
        vec![kmb_core::Op::SetFeatureData {
            id,
            data: serde_json::to_value(data).unwrap(),
        }],
    ))
    .unwrap();
}

#[test]
fn shell_opens_the_chosen_faces_and_hollows_a_chosen_body() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let block = block(&mut b);
    let sh = b.feature(
        SHELL,
        "シェル",
        shell(vec![top(block)], ShellDirection::Inside),
    );
    let mut doc = b.finish("シェル");
    if k.is_truck() {
        // truck のカーネルはシェルに対応していない: 理由つきで止まることだけ確かめる
        let report = doc.recompute(&registry(), &*k);
        match report.feature(sh).unwrap() {
            FeatureStatus::Error { message } => assert!(message.contains("シェル"), "{message}"),
            s => panic!("止まるはず: {s:?}"),
        }
        kmb_contract::skip!("truck はシェルを作れない(理由つきで止まるのは確かめた)");
    }
    // 上を開けた箱: 外 20×20×10 から内 16×16×8 を抜く
    let m = final_mass(&mut doc, &k);
    let open = 20.0 * 20.0 * 10.0 - 16.0 * 16.0 * 8.0;
    assert!(close(m.volume, open, 1e-4), "{} vs {open}", m.volume);
    // 面の名前: 外の面は元の名前のまま、内側の面は「:in:」、名前は重ならない
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    let FeatureStatus::Ok { outputs, .. } = report.feature(sh).unwrap() else {
        panic!()
    };
    let kmb_core::Value::Shape(h) = &outputs["body"] else {
        panic!()
    };
    let faces = k.faces(*h).unwrap();
    let names: Vec<String> = faces.iter().map(|f| f.name.clone()).collect();
    assert!(names.contains(&format!("f{}:start", block.0)), "{names:?}");
    // 開けた面は縁(20×20 − 16×16 の枠)になり、元の名前のまま残る
    let rim = faces
        .iter()
        .find(|f| f.name == format!("f{}:end", block.0))
        .unwrap();
    assert!(
        close(rim.area, 20.0 * 20.0 - 16.0 * 16.0, 1e-6),
        "{}",
        rim.area
    );
    assert!(names.iter().any(|n| n.contains(":in:")), "{names:?}");
    let mut uniq = names.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), names.len(), "{names:?}");

    // 上と側面を 1 枚ずつ開ける: 外 20×20×10 から内 16×18×8(側面の側へ 2 抜ける)
    let side = names
        .iter()
        .find(|n| n.starts_with(&format!("f{}:side", block.0)))
        .unwrap()
        .clone();
    let side = Ref::Face {
        feature: block,
        face: side,
    };
    set(
        &mut doc,
        sh,
        shell(vec![top(block), side], ShellDirection::Inside),
    );
    let m = final_mass(&mut doc, &k);
    let two = 20.0 * 20.0 * 10.0 - 16.0 * 18.0 * 8.0;
    assert!(close(m.volume, two, 1e-4), "{} vs {two}", m.volume);

    // ボディを選ぶと開けずに中を空洞にする
    set(
        &mut doc,
        sh,
        shell(vec![Ref::Body(block)], ShellDirection::Inside),
    );
    let m = final_mass(&mut doc, &k);
    let closed = 20.0 * 20.0 * 10.0 - 16.0 * 16.0 * 6.0;
    assert!(close(m.volume, closed, 1e-4), "{} vs {closed}", m.volume);

    // 外側へ厚みを付ける: 今の形が空洞になり、外へ 2 太る(角は丸くなるので箱の差より小さい)
    set(
        &mut doc,
        sh,
        shell(vec![top(block)], ShellDirection::Outside),
    );
    let m = final_mass(&mut doc, &k);
    let inner = 20.0 * 20.0 * 10.0;
    let boxy = 24.0 * 24.0 * 12.0 - inner;
    assert!(
        m.volume > 20.0 * 20.0 * 10.0 * 0.5 && m.volume < boxy + 1e-6,
        "{} vs {boxy}",
        m.volume
    );
}

#[test]
fn shell_refuses_mixed_or_empty_choices_with_a_reason() {
    let k = TestKernel::new();
    let reg = registry();
    let mut b = DocBuilder::new();
    let block = block(&mut b);
    let sh = b.feature(
        SHELL,
        "シェル",
        shell(vec![top(block), Ref::Body(block)], ShellDirection::Inside),
    );
    let mut doc = b.finish("シェル");
    let msg = |doc: &mut kmb_core::Document| match doc.recompute(&reg, &*k).feature(sh).unwrap() {
        FeatureStatus::Error { message } => message.clone(),
        s => panic!("止まるはず: {s:?}"),
    };
    if !k.is_truck() {
        assert!(msg(&mut doc).contains("ボディを選んだ時は面を選べません"));
    }
    set(&mut doc, sh, shell(vec![], ShellDirection::Inside));
    assert!(msg(&mut doc).contains("開ける面か、空洞にするボディを選んで"));
    let mut thick = shell(vec![top(block)], ShellDirection::Inside);
    thick.thickness = "0 mm".into();
    set(&mut doc, sh, thick);
    assert!(msg(&mut doc).contains("厚みは 0 より大きく"));
}

#[test]
fn shell_reads_one_face_or_a_list() {
    let one: Shell = serde_json::from_value(serde_json::json!({
        "faces": {"kind": "face", "feature": 3, "face": "f3:end"},
        "thickness": "1 mm"
    }))
    .unwrap();
    assert_eq!(one.faces.len(), 1);
    assert_eq!(one.direction, ShellDirection::Inside);
    let body: Shell =
        serde_json::from_value(serde_json::json!({"faces": 3, "thickness": "1 mm"})).unwrap();
    assert_eq!(body.faces, vec![Ref::Body(FeatureId(3))]);
}

/// 厚みで内側の面同士が重なる形(穴とポケットが近い見本の部品)は、崩れた形を出さずに
/// 理由を出して止まる。ポケットだけの板は殻になる。
#[test]
fn shell_stops_with_a_reason_where_the_inner_faces_would_overlap() {
    if TestKernel::new().is_truck() {
        kmb_contract::skip!("truck はシェルに対応していない");
    }
    let k = TestKernel::new();
    let reg = registry();
    let add_shell = |doc: &mut kmb_core::Document, body: FeatureId, face: String| {
        let id = doc.alloc_feature_id();
        doc.apply(kmb_core::Transaction::new(
            "シェル",
            vec![kmb_core::Op::InsertFeature {
                index: None,
                entry: kmb_core::FeatureEntry {
                    id,
                    type_id: SHELL.into(),
                    name: "シェル".into(),
                    suppressed: false,
                    pin_bodies: false,
                    data: serde_json::to_value(shell(
                        vec![Ref::Face {
                            feature: body,
                            face,
                        }],
                        ShellDirection::Inside,
                    ))
                    .unwrap(),
                },
            }],
        ))
        .unwrap();
        id
    };

    // ポケットだけの板: 上の面を開けると殻になる
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 60.0, 40.0);
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(s),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let end = format!("f{}:end", plate.0);
    let s2 = rect_sketch(
        &mut b,
        SketchPlane::face(plate, end.clone()),
        20.0,
        8.0,
        15.0,
        12.0,
    );
    let pocket = b.feature(
        EXTRUDE,
        "ポケット",
        Extrude {
            profile: Some(s2),
            distance: "4 mm".into(),
            direction: Direction::Reverse,
            operation: Operation::Cut,
            target: Some(plate),
            ..Default::default()
        },
    );
    let mut doc = b.finish("ポケット");
    let sh = add_shell(&mut doc, pocket, end);
    let report = doc.recompute(&reg, &*k);
    assert!(
        matches!(report.feature(sh), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        report.feature(sh)
    );
    let v = final_mass(&mut doc, &k).volume;
    assert!(v > 0.0 && v < 60.0 * 40.0 * 10.0 - 15.0 * 12.0 * 4.0, "{v}");

    // 見本の部品: 8 mm の穴とポケットが近く、2 mm の厚みで内側の面が重なる
    let mut part = kmb_samples::sample::reference_part();
    let report = part.doc.recompute(&reg, &*k);
    let &(last, h) = crate::visible_bodies(part.doc.data(), &reg, &report)
        .last()
        .unwrap();
    let top = k
        .faces(h)
        .unwrap()
        .into_iter()
        .filter(|f| f.planar.is_some_and(|p| p.normal[2] > 0.99))
        .max_by(|a, b| a.planar.unwrap().point[2].total_cmp(&b.planar.unwrap().point[2]))
        .unwrap()
        .name;
    let sh = add_shell(&mut part.doc, last, top);
    match part.doc.recompute(&reg, &*k).feature(sh).unwrap() {
        FeatureStatus::Error { message } => {
            assert!(message.contains("内側の面同士が重なる"), "{message}")
        }
        s => panic!("止まるはず: {s:?}"),
    }
}

/// 基準部品の最後に、ボディを選んだシェル(閉じた空洞。厚さ 1 mm)を足し、その後で板の右の側面を
/// 内へ 0.5 mm オフセットして削る(空洞は側面から 1 mm 内側で、削る所はその横)。
/// 削った後も空洞は残り、体積は 40 × 10 × 0.5 だけ減ること。
/// 画面の台本 `apps/kmb-app/tests/ui/空洞の横の切り取り.jcs` と同じ手順。
#[test]
fn a_sealed_void_survives_a_cut_beside_it() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck はシェルに対応していない");
    }
    let mut part = reference_part();
    let insert =
        |doc: &mut kmb_core::Document, type_id: &str, name: &str, data: serde_json::Value| {
            let id = doc.alloc_feature_id();
            doc.apply(kmb_core::Transaction::new(
                name,
                vec![kmb_core::Op::InsertFeature {
                    index: None,
                    entry: kmb_core::FeatureEntry {
                        id,
                        type_id: type_id.into(),
                        name: name.into(),
                        suppressed: false,
                        pin_bodies: false,
                        data,
                    },
                }],
            ))
            .unwrap();
            id
        };
    let whole = final_mass(&mut part.doc, &k).volume;
    let sh = insert(
        &mut part.doc,
        SHELL,
        "シェル",
        serde_json::to_value(Shell {
            faces: vec![Ref::Body(part.hole_pattern)],
            thickness: "1 mm".into(),
            direction: ShellDirection::Inside,
            tangent_chain: false,
        })
        .unwrap(),
    );
    let reg = registry();
    let report = part.doc.recompute(&reg, &*k);
    let bodies = kmb_core::visible_bodies(part.doc.data(), &reg, &report);
    assert!(
        matches!(report.feature(sh), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        report.feature(sh)
    );
    let h = bodies[0].1;
    let hollow = k.mass(h).unwrap().volume;
    assert!(hollow < whole - 1000.0, "空洞ができる: {hollow} vs {whole}");
    // 板の右の側面(x = 60)
    let right = k
        .faces(h)
        .unwrap()
        .into_iter()
        .find(|f| {
            f.planar
                .is_some_and(|p| p.normal[0] > 0.99 && (p.point[0] - 60.0).abs() < 1e-6)
        })
        .expect("右の側面")
        .name;
    eprintln!("右の側面 {right}、シェルの後の体積 {hollow:.3} mm³");
    insert(
        &mut part.doc,
        kmb_cad_modify::OFFSET_FACES,
        "面をオフセット",
        serde_json::json!({"faces": [{"kind": "face", "feature": sh.0, "face": right}], "distance": "-0.5 mm"}),
    );
    let m = final_mass(&mut part.doc, &k);
    let want = hollow - 40.0 * 10.0 * 0.5;
    assert!(
        close(m.volume, want, 1e-6),
        "体積 {} 理論値 {want}",
        m.volume
    );
    eprintln!("空洞の横を削った後の体積 {:.3} mm³", m.volume);
}

/// 機能を最後に足す。
fn insert(doc: &mut kmb_core::Document, type_id: &str, data: serde_json::Value) -> FeatureId {
    let id = doc.alloc_feature_id();
    doc.apply(kmb_core::Transaction::new(
        "足す",
        vec![kmb_core::Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: type_id.into(),
                name: type_id.into(),
                suppressed: false,
                pin_bodies: false,
                data,
            },
        }],
    ))
    .unwrap();
    id
}

/// 接線チェーン: 選んだ面に丸めた辺でなめらかにつながる面も
/// 開ける。外すと選んだ面だけ。全部の面を開けることになる時は、理由を出して断る。
#[test]
fn the_tangent_chain_opens_the_faces_joined_smoothly() {
    if TestKernel::new().is_truck() {
        kmb_contract::skip!("truck はシェルに対応していない");
    }
    let k = TestKernel::new();
    let reg = registry();
    let mut b = DocBuilder::new();
    let block = block(&mut b);
    let mut doc = b.finish("シェル");
    let report = doc.recompute(&reg, &*k);
    let FeatureStatus::Ok { outputs, .. } = report.feature(block).unwrap() else {
        panic!()
    };
    let kmb_core::Value::Shape(h) = &outputs["body"] else {
        panic!()
    };
    let faces = k.faces(*h).unwrap();
    let facing = |n: [f64; 3]| {
        faces
            .iter()
            .find(|f| {
                f.planar
                    .is_some_and(|p| (0..3).all(|i| close(p.normal[i], n[i], 1e-9)))
            })
            .unwrap()
            .name
            .clone()
    };
    let (front, right) = (facing([0.0, -1.0, 0.0]), facing([1.0, 0.0, 0.0]));
    // 前の面と右の面の間の縦の辺を丸める
    let fillet = insert(
        &mut doc,
        FILLET,
        serde_json::json!({"target": block.0, "edges": [[front, right]], "radius": "4 mm"}),
    );
    let mut data = shell(
        vec![Ref::Face {
            feature: fillet,
            face: front.clone(),
        }],
        ShellDirection::Inside,
    );
    let sh = insert(&mut doc, SHELL, serde_json::to_value(&data).unwrap());
    // 前の面だけを開けると、開けた縁が丸めの面に接して終わり、殻を作れない(理由を出して止まる)
    match doc.recompute(&reg, &*k).feature(sh).unwrap() {
        FeatureStatus::Error { message } => assert!(message.contains("シェル"), "{message}"),
        s => panic!("止まるはず: {s:?}"),
    }
    // 接線チェーンで、丸めと右の面も一緒に開ける: 上・下・後ろ・左の壁が残る
    data.tangent_chain = true;
    set(&mut doc, sh, data.clone());
    let chained = final_mass(&mut doc, &k).volume;
    let full = 20.0 * 20.0 * 10.0 - (4.0 * 4.0 - std::f64::consts::PI * 4.0) * 10.0;
    assert!(chained > 0.0 && chained < full * 0.6, "{chained} / {full}");

    // 全部の面を選ぶと、本体が残らないことを理由に断る(殻は丸めの後の形を読むので、その面の全部)
    let report = doc.recompute(&reg, &*k);
    let FeatureStatus::Ok { outputs, .. } = report.feature(fillet).unwrap() else {
        panic!()
    };
    let kmb_core::Value::Shape(rounded) = &outputs["body"] else {
        panic!()
    };
    let all: Vec<Ref> = k
        .faces(*rounded)
        .unwrap()
        .into_iter()
        .map(|f| Ref::Face {
            feature: fillet,
            face: f.name,
        })
        .collect();
    let msg = |doc: &mut kmb_core::Document| match doc.recompute(&reg, &*k).feature(sh).unwrap() {
        FeatureStatus::Error { message } => message.clone(),
        s => panic!("止まるはず: {s:?}"),
    };
    set(&mut doc, sh, shell(all, ShellDirection::Inside));
    assert!(msg(&mut doc).contains("全部の面を開けると本体が残りません"));
    // 古い文書(欄が無い)は切、画面から作った時の既定は入
    let old: Shell =
        serde_json::from_value(serde_json::json!({"faces": 3, "thickness": "1 mm"})).unwrap();
    assert!(!old.tangent_chain);
    let template = reg.descriptor(SHELL).unwrap().template.unwrap();
    assert_eq!(template["tangent_chain"], true);
}
