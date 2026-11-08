//! 点から点の移動の端を点の参照で持つ: 元の形やスケッチの寸法を変えると、動かしたボディが付いて来る。
//! 参照が切れたら座標へ戻さず、その移動だけを誤りにする。座標で持つ前からの文書はそのまま動く。
use super::modify_tests::set;
use super::tests::{TestKernel, assert_ok, registry};
use super::*;
use kmb_cad::PointRef;
use kmb_cad_modify::{MOVE, MOVE_TO, ModifyCommands, MoveBody};
use kmb_core::{
    Context, Extension, FeatureId, FeatureStatus, Op, Transaction, visible_bodies_before,
};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;
use serde_json::json;

/// (x, 0) から 30×20 の長方形を、高さ `height`(式)に押し出した板。
fn plate(b: &mut DocBuilder, x: f64, height: &str) -> FeatureId {
    let s = super::tests::rect_sketch(b, SketchPlane::Xy, x, 0.0, 30.0, 20.0);
    b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(s),
            distance: height.into(),
            ..Default::default()
        },
    )
}

/// 見えているボディの、機能 `id` の外形の箱。
fn bounds(doc: &mut kmb_core::Document, k: &TestKernel, id: FeatureId) -> ([f64; 3], [f64; 3]) {
    let reg = registry();
    let report = doc.recompute(&reg, &**k);
    assert_ok(&report);
    let (_, h) = visible_bodies_before(doc.data(), &reg, &report, None)
        .into_iter()
        .find(|(f, _)| *f == id)
        .unwrap_or_else(|| panic!("機能 {id} のボディが見えていない"));
    k.bounds(h).unwrap()
}

/// 機能 `before` の前に見えているボディ `id` の、位置 `at` にある角の名前。
fn corner(
    doc: &mut kmb_core::Document,
    k: &TestKernel,
    (id, before): (FeatureId, FeatureId),
    at: [f64; 3],
) -> String {
    let reg = registry();
    let report = doc.recompute(&reg, &**k);
    let (_, h) = visible_bodies_before(doc.data(), &reg, &report, Some(before))
        .into_iter()
        .find(|(f, _)| *f == id)
        .unwrap();
    kmb_core::vertices(&**k, h)
        .into_iter()
        .find(|v| (0..3).all(|i| (v.at[i] - at[i]).abs() < 1e-6))
        .unwrap_or_else(|| panic!("{at:?} に角が無い"))
        .name
}

/// 外形の箱はカーネルの許容差だけ外へ広がるので、1e-4 まで同じと見る。
fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4)
}

/// 確かめること: 板 B の角を板 A の上の角へ動かすと、A の高さを変えた時に B も付いて来る。
/// 先の点を A の辺の中点にしても同じ。点を借りた A は見えるボディに残る
#[test]
fn a_point_to_point_move_follows_the_other_body_when_its_size_changes() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    b.param("height", "10 mm");
    let a = plate(&mut b, 0.0, "height");
    let q = plate(&mut b, 50.0, "5 mm");
    let m = b.feature(MOVE, "点から点", json!({"body": q.0}));
    let mut doc = b.finish("点から点");
    let from = corner(&mut doc, &k, (q, m), [50.0, 0.0, 0.0]);
    let to = corner(&mut doc, &k, (a, m), [30.0, 20.0, 10.0]);
    let data = json!({"body": q.0, "mode": "point_to_point",
        "from_ref": {"kind": "vertex", "feature": q.0, "vertex": from},
        "to_ref": {"kind": "vertex", "feature": a.0, "vertex": to}});
    // 古い形と同じく座標も残せる。参照があれば座標は読まない
    let mut with_coords = data.clone();
    with_coords["to_point"] = json!([0, 0, 0]);
    doc.apply(set(m, with_coords)).unwrap();
    let lo = bounds(&mut doc, &k, m).0;
    assert!(near(lo, [30.0, 20.0, 10.0]), "{lo:?}");
    // 点を借りた A も見えるボディに残る
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    let shown: Vec<FeatureId> = visible_bodies_before(doc.data(), &reg, &report, None)
        .into_iter()
        .map(|(f, _)| f)
        .collect();
    assert!(shown.contains(&a) && shown.contains(&m), "{shown:?}");

    let id = doc.data().param_by_name("height").unwrap().id;
    doc.apply(Transaction::new(
        "高さ",
        vec![Op::SetParamExpr {
            id,
            expr: "25 mm".into(),
        }],
    ))
    .unwrap();
    let lo = bounds(&mut doc, &k, m).0;
    assert!(near(lo, [30.0, 20.0, 25.0]), "A の高さに付いて来る: {lo:?}");

    // 先の点を A の上の面と y = 0 の側面の境目の中点にする(高さ 25 の時は (15, 0, 25))
    let faces = {
        let report = doc.recompute(&reg, &*k);
        let (_, h) = visible_bodies_before(doc.data(), &reg, &report, None)
            .into_iter()
            .find(|(f, _)| *f == a)
            .unwrap();
        k.edges(h)
            .unwrap()
            .into_iter()
            .find(|e| near(k_mid(e.a, e.b), [15.0, 0.0, 25.0]))
            .expect("上の y = 0 の辺")
            .faces
    };
    let mut mid = data;
    mid["to_ref"] = json!({"kind": "midpoint", "feature": a.0, "faces": faces});
    doc.apply(set(m, mid)).unwrap();
    let lo = bounds(&mut doc, &k, m).0;
    assert!(near(lo, [15.0, 0.0, 25.0]), "{lo:?}");
}

fn k_mid(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| (a[i] + b[i]) / 2.0)
}

/// 確かめること: スケッチの点を先の点にすると、スケッチの寸法を変えた時に付いて来る。
#[test]
fn a_point_to_point_move_follows_a_sketch_point() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    b.param("gap", "40 mm");
    let q = plate(&mut b, 0.0, "5 mm");
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    let p = s.point_from(o, "gap", "10 mm", [40.0, 10.0]);
    let sk = b.sketch("先の点", s);
    let m = b.feature(MOVE, "点から点", json!({"body": q.0}));
    let mut doc = b.finish("スケッチの点");
    doc.apply(set(
        m,
        json!({"body": q.0, "mode": "point_to_point", "from_point": [0, 0, 0],
            "to_ref": {"kind": "sketch", "sketch": sk.0, "point": p.0}}),
    ))
    .unwrap();
    let lo = bounds(&mut doc, &k, m).0;
    assert!(near(lo, [40.0, 10.0, 0.0]), "{lo:?}");
    let id = doc.data().param_by_name("gap").unwrap().id;
    doc.apply(Transaction::new(
        "間",
        vec![Op::SetParamExpr {
            id,
            expr: "70 mm".into(),
        }],
    ))
    .unwrap();
    let lo = bounds(&mut doc, &k, m).0;
    assert!(
        near(lo, [70.0, 10.0, 0.0]),
        "スケッチの寸法に付いて来る: {lo:?}"
    );
}

/// 確かめること: 参照が切れると座標へ戻さず、その移動だけを誤りにする(元のボディは止めない)。
/// まだ点を選んでいない時も理由を出して止まる
#[test]
fn a_broken_point_reference_stops_only_the_move() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let a = plate(&mut b, 0.0, "10 mm");
    let m = b.feature(
        MOVE,
        "点から点",
        json!({"body": a.0, "mode": "point_to_point", "from_point": [0, 0, 0], "to_point": [5, 5, 5],
            "to_ref": {"kind": "vertex", "feature": a.0, "vertex": "無い角"}}),
    );
    let mut doc = b.finish("切れた参照");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    assert!(matches!(report.feature(a), Some(FeatureStatus::Ok { .. })));
    let err = format!("{:?}", report.feature(m));
    assert!(
        err.contains("無い角") && err.contains("選び直して"),
        "{err}"
    );

    let mut pick = doc.data().feature(m).unwrap().data.clone();
    pick["to_ref"] = json!({"kind": "pick"});
    doc.apply(set(m, pick)).unwrap();
    let report = doc.recompute(&reg, &*k);
    let err = format!("{:?}", report.feature(m));
    assert!(err.contains("先の点をまだ選んでいません"), "{err}");
}

/// 確かめること: 座標で持つ前からの文書は、同じ形のまま読み書きでき、座標のまま動く。
/// 知らない種類の点は読まない
#[test]
fn coordinate_moves_keep_their_saved_form() {
    let old = json!({"body": 2, "mode": "point_to_point", "from_point": [0.0, 0.0, 0.0], "to_point": [0.0, 40.0, 0.0], "x": "0 mm", "y": "0 mm", "z": "0 mm", "axis": [0.0, 0.0, 1.0], "angle": "0 deg"});
    let m: MoveBody = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&m).unwrap(),
        old,
        "前の形のまま書き戻す"
    );
    let refs = json!({"body": 2, "mode": "point_to_point", "x": "0 mm", "y": "0 mm", "z": "0 mm", "axis": [0.0, 0.0, 1.0], "angle": "0 deg",
        "from_ref": {"kind": "pick"},
        "to_ref": {"kind": "center", "feature": 3, "faces": ["f3:end", "f3:side:e0"]}});
    let m: MoveBody = serde_json::from_value(refs.clone()).unwrap();
    assert_eq!(serde_json::to_value(&m).unwrap(), refs);
    let mut bad = refs;
    bad["to_ref"] = json!({"kind": "somewhere"});
    assert!(serde_json::from_value::<MoveBody>(bad).is_err());
}

/// 確かめること: 「先の点を選ぶ」はこの移動より前の点の候補を出し、選んだ点を参照で入れる。
/// 候補に無い点(後ろの機能や無い角)は誤り
#[test]
fn picking_the_destination_point_stores_the_reference() {
    let mut b = DocBuilder::new();
    let a = plate(&mut b, 0.0, "10 mm");
    let q = plate(&mut b, 50.0, "5 mm");
    let m = b.feature(MOVE, "点から点", json!({"body": q.0}));
    let later = plate(&mut b, 100.0, "5 mm");
    let mut doc = b.finish("選ぶ");
    let reg = registry();
    let kernel: std::sync::Arc<dyn kmb_core::Kernel> =
        std::sync::Arc::new(kmb_kernel_smlib::SmlibKernel::new());
    let report = doc.recompute(&reg, kernel.as_ref());
    let data = doc.data().clone();
    let none = json!({});
    let cx = Context {
        selected: Some(m),
        args: &none,
        ..Context::new(&data, &report, &reg, &kernel)
    };
    let handles = ModifyCommands.handles(MOVE_TO, &cx).unwrap();
    let choices = &handles[0].choices;
    let kinds: Vec<PointRef> = choices
        .iter()
        .map(|c| serde_json::from_value(c.value.clone()).unwrap())
        .collect();
    assert!(
        kinds
            .iter()
            .any(|p| matches!(p, PointRef::Vertex { feature, .. } if *feature == a))
    );
    assert!(
        kinds
            .iter()
            .any(|p| matches!(p, PointRef::Midpoint { feature, .. } if *feature == a))
    );
    assert!(
        kinds.iter().all(|p| p.feature() != later),
        "後ろの機能の点は出さない"
    );
    let chosen = choices
        .iter()
        .find(|c| matches!(serde_json::from_value::<PointRef>(c.value.clone()), Ok(PointRef::Vertex { feature, .. }) if feature == a))
        .unwrap()
        .value
        .clone();
    let args = json!({"point": chosen});
    let cx = Context { args: &args, ..cx };
    let Some(Ok(kmb_core::Outcome::Change { edits, .. })) = ModifyCommands.run(MOVE_TO, &cx) else {
        panic!("先の点を選べない");
    };
    let kmb_core::Edit::SetData { id, data: new } = &edits[0] else {
        panic!();
    };
    assert_eq!(*id, m);
    assert_eq!(new["to_ref"], chosen);
    assert_eq!(new["mode"], "point_to_point");

    for wrong in [
        json!({"kind": "vertex", "feature": later.0, "vertex": "x"}),
        json!({"kind": "vertex", "feature": a.0, "vertex": "無い角"}),
        json!({"kind": "vertex", "feature": a.0}),
    ] {
        let args = json!({"point": wrong});
        let cx = Context { args: &args, ..cx };
        assert!(
            matches!(ModifyCommands.run(MOVE_TO, &cx), Some(Err(_))),
            "{wrong}"
        );
    }
}
