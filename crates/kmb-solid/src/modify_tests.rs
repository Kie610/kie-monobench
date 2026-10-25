//! ボディの分割、ボディの移動・コピー・尺度、面のオフセット。

use super::tests::{TestKernel, assert_ok, close, registry};
use super::*;
use kmb_cad_modify::{MOVE, OFFSET_FACES, SCALE, SPLIT, SPLIT_PART};
use kmb_core::{FeatureId, visible_bodies_before};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;
use serde_json::json;

/// (0, 0) から 30×20 の長方形を、高さ 10 に押し出した板。
fn plate(b: &mut DocBuilder) -> FeatureId {
    let s = super::tests::rect_sketch(b, SketchPlane::Xy, 0.0, 0.0, 30.0, 20.0);
    b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(s),
            distance: "10 mm".into(),
            ..Default::default()
        },
    )
}

/// 機能 `id` のデータを置き換える変更。
pub(crate) fn set(id: FeatureId, data: serde_json::Value) -> kmb_core::Transaction {
    kmb_core::Transaction::new("変更", vec![kmb_core::Op::SetFeatureData { id, data }])
}

/// 見えているボディ 1 つ(最後の機能の番号、体積、外形の箱)。
type BodyInfo = (FeatureId, f64, ([f64; 3], [f64; 3]));

/// 見えているボディ(最後の機能の番号)と、その体積・外形の箱。
fn bodies(doc: &mut kmb_core::Document, k: &TestKernel) -> Vec<BodyInfo> {
    let reg = registry();
    let report = doc.recompute(&reg, &**k);
    assert_ok(&report);
    visible_bodies_before(doc.data(), &reg, &report, None)
        .into_iter()
        .map(|(id, h)| {
            let m = k.mass(h).unwrap();
            (id, m.volume, (m.min, m.max))
        })
        .collect()
}

/// 板 1 枚の分割を、割る平面だけを替えて確かめる(文書は 1 回だけ作る)。
/// - 通る点を省くと外形の中心で厚みの半分を割り、法線の側は元のボディの名前を継ぐ。端の平面は止まる
/// - 通る点と法線で x = 10 を割っても両側が残る。別のボディの平らな面も選べ、横切らなければ止まる
#[test]
fn split_cuts_a_plate_by_a_plane_and_stops_when_the_plane_misses_it() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は平面での分割を作れない");
    }
    let mut b = DocBuilder::new();
    let p = plate(&mut b);
    // 通る点を省くと外形の中心: 厚みの半分(z = 5)で表と裏に割る
    let half = json!({"body": p.0, "normal": [0, 0, 1]});
    let s = b.feature(SPLIT, "分割", half.clone());
    let r = b.feature(SPLIT_PART, "分割の残り", json!({"of": s.0}));
    let mut doc = b.finish("分割");
    let v = bodies(&mut doc, &k);
    assert_eq!(v.len(), 2, "元の板は消え、2 つのボディになる: {v:?}");
    let up = v.iter().find(|x| x.0 == s).expect("法線の側は分割のボディ");
    let down = v.iter().find(|x| x.0 == r).expect("反対の側は残りのボディ");
    assert!(
        close(up.1, 3000.0, 1e-6) && close(down.1, 3000.0, 1e-6),
        "{v:?}"
    );
    assert!((up.2.0[2] - 5.0).abs() < 1e-6 && (up.2.1[2] - 10.0).abs() < 1e-6);
    assert!((down.2.1[2] - 5.0).abs() < 1e-6);
    // 名前: 法線の側は元のボディ(ボディ1)、反対の側は新しいボディ
    let reg = registry();
    let names = kmb_core::body_labels(doc.data(), &reg, &[s, r]);
    assert_eq!(names, vec!["ボディ1", "ボディ2"]);

    // 板の端面(x = 0)を 10 だけ内へずらした平面ではなく、端面そのものは横切らないので、
    // 通る点と法線で x = 10 の平面にする
    let x10 = json!({"body": p.0, "origin": [10, 0, 0], "normal": [1, 0, 0]});
    doc.apply(set(s, x10.clone())).unwrap();
    let v = bodies(&mut doc, &k);
    let vols: Vec<f64> = v.iter().map(|x| x.1).collect();
    assert_eq!(v.len(), 2);
    assert!(close(vols[0] + vols[1], 6000.0, 1e-6), "{vols:?}");
    assert!(vols.iter().any(|x| close(*x, 4000.0, 1e-6)), "{vols:?}");

    // 割った面は、別のボディの平らな面(板の上面)でも選べる: 上面は横切らないので止まる
    let mut data = x10;
    data["plane_ref"] = json!({"kind": "face", "feature": p.0, "face": format!("f{}:end", p.0)});
    doc.apply(set(s, data)).unwrap();
    let report = doc.recompute(&reg, &*k);
    assert!(format!("{:?}", report.feature(s)).contains("横切っていません"));

    // 原点の YZ 平面は板の端なので横切らない: 理由を出して止まる
    let mut data = half;
    data["plane_ref"] = json!({"kind": "origin_yz"});
    doc.apply(set(s, data)).unwrap();
    let report = doc.recompute(&reg, &*k);
    let err = format!("{:?}", report.feature(s));
    assert!(err.contains("横切っていません"), "{err}");
}

#[test]
fn move_translates_rotates_and_copies_a_body() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let p = plate(&mut b);
    let m = b.feature(MOVE, "移動", json!({"body": p.0, "x": "5 mm", "z": "2 mm"}));
    let mut doc = b.finish("移動");
    let v = bodies(&mut doc, &k);
    assert_eq!(v.len(), 1);
    let (lo, hi) = v[0].2;
    assert!(
        (lo[0] - 5.0).abs() < 1e-6 && (lo[2] - 2.0).abs() < 1e-6 && (hi[0] - 35.0).abs() < 1e-6
    );

    // 回転: 原点の Z 軸まわりに 90 度。X の 0..30 が Y の 0..30 になる
    let mut data =
        json!({"body": p.0, "mode": "rotate", "axis_ref": {"kind": "origin_z"}, "angle": "90 deg"});
    doc.apply(set(m, data.clone())).unwrap();
    let (lo, hi) = bodies(&mut doc, &k)[0].2;
    assert!(
        (hi[1] - 30.0).abs() < 1e-6 && (lo[0] + 20.0).abs() < 1e-6,
        "{lo:?} {hi:?}"
    );

    // コピー: 元の板も残り、ボディが 2 つになる
    data = json!({"body": p.0, "mode": "point_to_point", "from_point": [0, 0, 0], "to_point": [0, 40, 0], "copy": true});
    doc.apply(set(m, data)).unwrap();
    let v = bodies(&mut doc, &k);
    assert_eq!(v.len(), 2, "{v:?}");
    let copy = v.iter().find(|x| x.0 == m).unwrap();
    assert!((copy.2.0[1] - 40.0).abs() < 1e-6);
    assert!(v.iter().any(|x| x.0 == p));
}

/// 尺度(M25): 均一は原点か基準の点を中心に 3 方向へ、不均一は軸ごとに大きさを変える。
/// 円柱を不均一に伸ばすと楕円柱になり、体積は倍率の積で増える。倍率に単位や 0 を書くと止まる
#[test]
fn scale_grows_a_body_about_a_point_and_rejects_bad_factors() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let p = plate(&mut b);
    let sc = b.feature(SCALE, "尺度", json!({"body": p.0, "factor": "2"}));
    let mut doc = b.finish("尺度");
    let v = bodies(&mut doc, &k);
    assert_eq!(v.len(), 1, "尺度は元のボディを置き換える: {v:?}");
    assert_eq!(v[0].0, sc);
    assert!(close(v[0].1, 6000.0 * 8.0, 1e-6), "{v:?}");
    let (lo, hi) = v[0].2;
    assert!(
        lo.iter().all(|x| x.abs() < 1e-6) && (hi[0] - 60.0).abs() < 1e-6,
        "原点が中心: {lo:?} {hi:?}"
    );
    // 面の名前は尺度の前と同じ(後の機能が上面を指したままでいられる)
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    let (_, h) = visible_bodies_before(doc.data(), &reg, &report, None)[0];
    let faces = k.faces(h).unwrap();
    assert!(
        faces.iter().any(|f| f.name == format!("f{}:end", p.0)),
        "{faces:?}"
    );

    // 基準の点を板の中心にして半分: 中心は動かず、厚みは 5
    doc.apply(set(
        sc,
        json!({"body": p.0, "factor": "0.5", "point": [15, 10, 5]}),
    ))
    .unwrap();
    let v = bodies(&mut doc, &k);
    let (lo, hi) = v[0].2;
    assert!(close(v[0].1, 6000.0 / 8.0, 1e-6), "{v:?}");
    assert!(
        (lo[0] - 7.5).abs() < 1e-6 && (hi[2] - 7.5).abs() < 1e-6,
        "{lo:?} {hi:?}"
    );

    // 不均一: Z だけ 3 倍
    let data = json!({"body": p.0, "mode": "non_uniform", "x": "1", "y": "1", "z": "3"});
    doc.apply(set(sc, data)).unwrap();
    let v = bodies(&mut doc, &k);
    assert!(
        close(v[0].1, 18000.0, 1e-6) && (v[0].2.1[2] - 30.0).abs() < 1e-6,
        "{v:?}"
    );

    // 倍率に長さの単位、0 は止まる
    for bad in ["2 mm", "0"] {
        doc.apply(set(sc, json!({"body": p.0, "factor": bad})))
            .unwrap();
        let report = doc.recompute(&reg, &*k);
        let err = format!("{:?}", report.feature(sc));
        assert!(err.contains("尺度係数"), "{bad}: {err}");
    }
}

/// 円柱(直径 20、高さ 10)を X だけ 2 倍に伸ばすと楕円柱になる(体積は 2 倍、X の幅は 40)。
#[test]
fn non_uniform_scale_turns_a_cylinder_into_an_elliptic_one() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.circle(o, "20 mm", 10.0);
    let cs = b.feature(kmb_sketch::SKETCH, "円", s.build());
    let cyl = b.feature(
        EXTRUDE,
        "円柱",
        Extrude {
            profile: Some(cs),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    b.feature(
        SCALE,
        "尺度",
        json!({"body": cyl.0, "mode": "non_uniform", "x": "2", "y": "1", "z": "1"}),
    );
    let mut doc = b.finish("楕円柱");
    let v = bodies(&mut doc, &k);
    let want = std::f64::consts::PI * 100.0 * 10.0 * 2.0;
    assert!(close(v[0].1, want, 1e-3), "{} と {want}", v[0].1);
    let (lo, hi) = v[0].2;
    assert!(
        (hi[0] - lo[0] - 40.0).abs() < 1e-3 && (hi[1] - lo[1] - 20.0).abs() < 1e-3,
        "{lo:?} {hi:?}"
    );
}

#[test]
fn offset_faces_widens_a_hole_and_keeps_the_face_name() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は面のオフセットを作れない");
    }
    let part = kmb_samples::reference_part();
    let mut doc = part.doc;
    let v0 = bodies(&mut doc, &k);
    let (body, vol0, _) = *v0.last().unwrap();
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    let h = visible_bodies_before(doc.data(), &reg, &report, None)
        .last()
        .unwrap()
        .1;
    let hole = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == "solid.hole@1")
        .unwrap()
        .id;
    let faces = k.faces(h).unwrap();
    let wall = faces
        .iter()
        .find(|f| kmb_core::face_feature(&f.name) == Some(hole) && f.planar.is_none())
        .unwrap();
    let mut doc2 = doc;
    let off = doc2
        .apply_edits(
            "面のオフセット",
            vec![kmb_core::Edit::Insert {
                type_id: OFFSET_FACES.into(),
                name: "面のオフセット".into(),
                data: json!({"faces": [{"kind": "face", "feature": body.0, "face": wall.name}], "distance": "-0.5 mm"}),
            }],
        )
        .unwrap()[0];
    let v = bodies(&mut doc2, &k);
    let (b2, vol, _) = *v.last().unwrap();
    assert_eq!(b2, off);
    assert!(vol < vol0 - 1.0, "穴が広がって軽くなる: {vol0} → {vol}");
    // 動かした面は元の名前のまま(後の機能が指し続けられる)
    let report = doc2.recompute(&reg, &*k);
    let h3 = visible_bodies_before(doc2.data(), &reg, &report, None)
        .last()
        .unwrap()
        .1;
    assert!(k.faces(h3).unwrap().iter().any(|f| f.name == wall.name));
}
