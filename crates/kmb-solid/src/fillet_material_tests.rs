//! フィレットの半径の種類、物理材料と外観、質量と重心。

use super::tests::{EdgeBench, TestKernel, edge_feature_status, with_edge_feature};
use super::*;
use kmb_core::{FeatureStatus, Op, Transaction};
use kmb_samples::sample::reference_part;
use serde_json::json;
use std::f64::consts::PI;

/// 基準部品の板の縦の辺(外周の角)。
fn corner(a: u32, b: u32) -> serde_json::Value {
    let p = reference_part().plate.0;
    json!({"kind": "edge", "feature": 0, "faces": [format!("f{p}:side:e{a}"), format!("f{p}:side:e{b}")]})
}

/// 板の高さ 10 mm の縦の辺を半径 r で丸めて削れる体積。
fn rounded(r: f64) -> f64 {
    r * r * (1.0 - PI / 4.0) * 10.0
}

/// カーネルの丸め(SMLib)でだけ作れるので、truck の回では飛ばす。
fn smlib() -> bool {
    !TestKernel::new().is_truck()
}

/// 台の最後のフィレットを、カーネルの丸めの 1 行 `row` に替えて削れる体積。
fn removed(bench: &mut EdgeBench, row: serde_json::Value) -> f64 {
    bench.before - bench.after(FILLET, json!({"method": "blend", "rows": [row]}))
}

/// 基準部品の板の角 1 本を、半径の種類を替えながら丸める(部品は 1 回だけ作り、フィレットだけ替える)。
/// - 弦の長さ c は、直角の辺を半径 c/√2 で丸める
/// - 可変の半径は始めから終わりの半径へ変わり、終わりが空なら一定と同じ
/// - 曲率の連続(G2)の行は、G1 と違う断面で丸める
/// - 途中を細くしても板は残り、削る量は両端の半径より少ない
#[test]
fn radius_kinds_and_g2_round_a_corner_with_the_expected_volume() {
    if !smlib() {
        kmb_contract::skip!("truck は丸めの半径の種類と G2 を作れない");
    }
    let mut bench = EdgeBench::reference();
    // 弦の長さ
    let got = removed(
        &mut bench,
        json!({"edges": corner(5, 6), "radius": format!("{} mm", 2.0 * 2f64.sqrt()), "kind": "chord"}),
    );
    assert!(
        (got - rounded(2.0)).abs() < 0.05,
        "{got} は {} のはず",
        rounded(2.0)
    );

    // 可変の半径
    let got = removed(
        &mut bench,
        json!({"edges": corner(5, 6), "radius": "1 mm", "kind": "variable", "end_radius": "3 mm"}),
    );
    // 半径が直線で変わる: (r0² + r0·r1 + r1²) / 3 の平均の断面
    let want = (1.0 - PI / 4.0) * 10.0 * (1.0 + 3.0 + 9.0) / 3.0;
    assert!((got - want).abs() < 0.1, "{got} は {want} のはず");
    // 終わりの半径が空なら一定と同じ
    let same = removed(
        &mut bench,
        json!({"edges": corner(5, 6), "radius": "2 mm", "kind": "variable"}),
    );
    assert!((same - rounded(2.0)).abs() < 0.05, "{same}");

    // G2
    let g1 = removed(&mut bench, json!({"edges": corner(5, 6), "radius": "2 mm"}));
    let g2 = removed(
        &mut bench,
        json!({"edges": corner(5, 6), "radius": "2 mm", "continuity": "g2"}),
    );
    assert!((g1 - rounded(2.0)).abs() < 0.05, "{g1}");
    assert!(g2 > 0.0 && (g2 - g1).abs() > 1e-3, "G1 {g1} G2 {g2}");

    // 不具合(2026-10-19 の写真撮りで見つけた): 半径 1 mm・途中の半径 0.5 = 0.5 mm にすると
    // 「計算できた」のに板が細い帯だけになった
    let got = removed(
        &mut bench,
        json!({
            "edges": corner(5, 6), "radius": "1 mm", "kind": "variable", "mid_radii": "0.5 = 0.5 mm"
        }),
    );
    assert!(
        got > rounded(0.5) * 0.9 && got < rounded(1.0) * 1.05,
        "{got} は {} と {} の間のはず",
        rounded(0.5),
        rounded(1.0)
    );
}

#[test]
fn mid_radii_take_expressions_and_follow_a_renamed_parameter() {
    if !smlib() {
        kmb_contract::skip!("truck は途中の半径を変える丸めを作れない");
    }
    let (before, after, mut doc) = with_edge_feature(
        FILLET,
        json!({"target": 0, "method": "blend", "rows": [{
            "edges": corner(5, 6), "radius": "1 mm", "kind": "variable", "end_radius": "1 mm",
            "mid_radii": "0.5 = 1 mm * 3"
        }]}),
    );
    // 真ん中が太い分だけ、一定の 1 mm より多く削る
    assert!(before - after > rounded(1.0) * 2.0, "{before} {after}");
    // 途中の半径の式のパラメータは、名前を変えると付いていく
    let id = doc.data().features.last().unwrap().id;
    let mut data = doc.data().feature(id).unwrap().data.clone();
    data["rows"][0]["mid_radii"] = json!("0.5 = mid_r");
    let pid = doc.alloc_param_id();
    doc.apply(Transaction::new(
        "式",
        vec![
            Op::AddParam {
                index: None,
                param: kmb_core::Param {
                    id: pid,
                    name: "mid_r".into(),
                    expr: "3 mm".into(),
                    comment: String::new(),
                    unit: Default::default(),
                },
            },
            Op::SetFeatureData { id, data },
        ],
    ))
    .unwrap();
    let reg = super::tests::registry();
    let f = reg
        .instantiate(FILLET, &doc.data().feature(id).unwrap().data)
        .unwrap()
        .unwrap();
    let renamed = f.rename_param("mid_r", "bulge").unwrap().expect("変わる");
    assert_eq!(renamed["rows"][0]["mid_radii"], json!("0.5 = bulge"));
    assert!(
        f.inputs()
            .unwrap()
            .iter()
            .any(|i| matches!(i, kmb_core::InputRef::Param { name } if name == "mid_r"))
    );
}

#[test]
fn bad_mid_radii_and_cut_method_are_explained() {
    let status = edge_feature_status(
        FILLET,
        json!({"target": 0, "method": "blend", "rows": [{
            "edges": corner(5, 6), "radius": "1 mm", "kind": "variable", "mid_radii": "1.5 = 2 mm"
        }]}),
    );
    match status {
        FeatureStatus::Error { message } => assert!(message.contains("位置 = 半径"), "{message}"),
        s => panic!("{s:?}"),
    }
    // 前の文書の作り方(柱で削る)では、一定の半径だけ
    let status = edge_feature_status(
        FILLET,
        json!({"target": 0, "rows": [{"edges": corner(5, 6), "radius": "1 mm", "kind": "chord"}]}),
    );
    match status {
        FeatureStatus::Error { message } => {
            assert!(message.contains("カーネルの丸め"), "{message}")
        }
        s => panic!("{s:?}"),
    }
}

#[test]
fn a_body_material_gives_mass_and_undoes() {
    let k = TestKernel::new();
    let mut part = reference_part();
    let m = super::tests::final_mass(&mut part.doc, &k);
    let root = part.plate;
    let mut mats = kmb_look::material::BodyMaterials::default();
    mats.set(root, Some("アルミ"));
    let op = part.doc.data().section_op(&mats).unwrap();
    part.doc.apply(Transaction::new("材料", vec![op])).unwrap();
    assert_eq!(
        kmb_look::material::of(part.doc.data()).get(root),
        Some("アルミ")
    );
    let al = kmb_look::material::phys_material("アルミ").unwrap();
    let g = kmb_look::material::mass_of(m.volume, al.density);
    assert!((g - m.volume * 2.70e-3).abs() < 1e-9);
    // 保存して読み直しても残る
    let text = part.doc.to_json();
    let back = kmb_core::Document::from_json(&text).unwrap();
    assert_eq!(
        kmb_look::material::of(back.data()),
        kmb_look::material::of(part.doc.data())
    );
    assert!(part.doc.undo());
    assert!(kmb_look::material::of(part.doc.data()).bodies.is_empty());
    // 重心は外形の箱の中
    if let Some(c) = m.centroid {
        for i in 0..3 {
            assert!(c[i] > m.min[i] && c[i] < m.max[i], "{c:?}");
        }
    } else {
        assert!(k.is_truck(), "SMLib は重心を出す");
    }
}

#[test]
fn a_thinner_middle_on_a_long_top_edge_keeps_the_plate() {
    if !smlib() {
        kmb_contract::skip!("truck は途中の半径を変える丸めを作れない");
    }
    let k = TestKernel::new();
    let reg = super::tests::registry();
    let mut failed = Vec::new();
    for (r, mid, low, high) in [
        ("1 mm", "0.5 = 0.5 mm", 0.5_f64, 1.0_f64),
        ("1 mm", "0.5 = 0.8 mm", 0.8, 1.0),
        ("1.2 mm", "0.5 = 0.6 mm", 0.6, 1.2),
        ("3 mm", "0.5 = 2 mm", 2.0, 3.0),
        ("1 mm", "0.25 = 0.6 mm; 0.75 = 0.8 mm", 0.6, 1.0),
        ("1 mm", "0.5 = 2 mm", 1.0, 2.0),
    ] {
        let mut b = kmb_samples::sample::DocBuilder::new();
        let s =
            super::tests::rect_sketch(&mut b, kmb_sketch::SketchPlane::Xy, 0.0, 0.0, 60.0, 40.0);
        let plate = b.feature(
            EXTRUDE,
            "板",
            Extrude {
                profile: Some(s),
                distance: "8 mm".into(),
                ..Default::default()
            },
        );
        let mut doc = b.finish("板");
        let report = doc.recompute(&reg, &*k);
        let body = kmb_core::visible_bodies(doc.data(), &reg, &report)[0].1;
        // 手前(y = 0)の上の辺
        let e = k
            .edges(body)
            .unwrap()
            .into_iter()
            .find(|e| e.a[1].abs() < 1e-6 && e.b[1].abs() < 1e-6 && e.a[2] > 7.9 && e.b[2] > 7.9)
            .expect("手前の上の辺");
        let mut data = json!({"target": plate.0, "rows": [{
            "edges": [{"kind": "edge", "feature": plate.0, "faces": e.faces}],
            "radius": r, "kind": "variable", "mid_radii": mid,
        }]});
        reg.descriptor(FILLET)
            .unwrap()
            .fill_create_defaults(&mut data);
        let id = doc.alloc_feature_id();
        doc.apply(Transaction::new(
            "丸め",
            vec![Op::InsertFeature {
                index: None,
                entry: kmb_core::FeatureEntry {
                    id,
                    type_id: FILLET.into(),
                    name: "丸め".into(),
                    data,
                    suppressed: false,
                    pin_bodies: false,
                },
            }],
        ))
        .unwrap();
        let report = doc.recompute(&reg, &*k);
        let status = report.feature(id).unwrap();
        if !matches!(status, FeatureStatus::Ok { .. }) {
            failed.push(format!("半径 {r}・途中 {mid}: {status:?}"));
            eprintln!("失敗: {}", failed.last().unwrap());
            continue;
        }
        let bodies = kmb_core::visible_bodies(doc.data(), &reg, &report);
        let v = k.volume(bodies[0].1).unwrap();
        let plate_v = 60.0 * 40.0 * 8.0;
        let factor = (1.0 - PI / 4.0) * 60.0;
        let removed = plate_v - v;
        eprintln!("半径 {r}・途中 {mid}: 削れた体積 {removed:.6} mm³");
        if removed < low * low * factor - 0.1 || removed > high * high * factor + 0.1 {
            failed.push(format!("半径 {r}・途中 {mid}: 削れた体積 {removed}"));
        }
        let mesh = k.mesh(bodies[0].1, 0.005).unwrap();
        for point in mid.split(';') {
            let (at, radius) = point.trim().split_once('=').unwrap();
            let at: f64 = at.trim().parse().unwrap();
            let radius: f64 = radius.trim().strip_suffix(" mm").unwrap().parse().unwrap();
            let inner = radius * 0.4;
            let outer = radius * 0.2;
            let kept = mesh.contains([60.0 * at, inner, 8.0 - inner]);
            let cut = !mesh.contains([60.0 * at, outer, 8.0 - outer]);
            eprintln!("位置 {at} 半径 {radius}: 内側 {kept}・外側を削る {cut}");
            if !kept || !cut {
                failed.push(format!(
                    "半径 {r}・途中 {mid}: 位置 {at} の断面が半径 {radius} ではありません"
                ));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// 円の辺でも途中の半径を変え、閉じた形と削れる体積を確かめる。
#[test]
fn variable_fillet_on_a_round_rim_keeps_the_cylinder() {
    if !smlib() {
        kmb_contract::skip!("truck は途中の半径を変える丸めを作れない");
    }
    use kmb_core::{
        BlendLaw, EdgeBlend, Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment,
    };
    let k = TestKernel::new();
    let profile = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: ProfileLoop {
                edges: vec![ProfileEdge {
                    name: "rim".into(),
                    segment: Segment::Circle {
                        center: [0.0, 0.0],
                        radius: 10.0,
                    },
                    reversed: false,
                }],
            },
            holes: vec![],
        }],
    };
    let body = k.extrude(&profile, 0.0, 8.0, "cylinder").unwrap();
    let edge = k
        .arc_edges(body)
        .unwrap()
        .into_iter()
        .find(|e| e.a[2] > 7.9)
        .expect("上の円の辺");
    let arc = edge.arc.unwrap();
    let mut blend = EdgeBlend::new(edge.faces, edge.a, 1.0, false);
    blend.law = BlendLaw::Variable {
        start: edge.a,
        radii: vec![(0.0, 1.0), (0.5, 0.5), (1.0, 1.0)],
    };
    let result = k
        .blend_edges(body, &[blend], "fillet")
        .expect("円の辺を丸める");
    let removed = k.volume(body).unwrap() - k.volume(result).unwrap();
    let max = 2.0 * PI * arc.radius * (1.0 - PI / 4.0);
    eprintln!("円の辺: 削れた体積 {removed:.6} mm³");
    assert!(removed > 0.0 && removed < max + 0.1, "{removed}");
    let mesh = k.mesh(result, 0.1).unwrap();
    assert!(!mesh.triangles.is_empty());
}
