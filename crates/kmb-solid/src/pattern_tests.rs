//! 面取りの 2 つの距離・距離と角度、格子のパターン、ボディのパターン・ミラー、
//! ミラーの平面の選び方、切り取りとシェルの予告(`tool_preview`)。

use super::modify_tests::set;
use super::tests::{EdgeBench, TestKernel, close, final_mass, rect_sketch, registry};
use super::*;
use kmb_core::{FeatureId, FeatureStatus, Ref, ToolPreview};
use kmb_samples::sample::DocBuilder;
use kmb_sketch::SketchPlane;
use serde_json::json;

/// (x, y) から w×h の長方形を、高さ `d` に押し出した箱。
fn block(b: &mut DocBuilder, x: f64, y: f64, w: f64, h: f64, d: f64) -> FeatureId {
    let s = rect_sketch(b, SketchPlane::Xy, x, y, w, h);
    b.feature(
        EXTRUDE,
        "箱",
        Extrude {
            profile: Some(s),
            distance: format!("{d} mm"),
            ..Default::default()
        },
    )
}

/// 箱 `target` に、(x, y) から w×h の長方形を高さ `d` まで押し出して足す(引く)。
fn boss(
    b: &mut DocBuilder,
    target: FeatureId,
    op: Operation,
    [x, y, w, h, d]: [f64; 5],
) -> FeatureId {
    let s = rect_sketch(b, SketchPlane::Xy, x, y, w, h);
    b.feature(
        EXTRUDE,
        "出っ張り",
        Extrude {
            profile: Some(s),
            distance: format!("{d} mm"),
            operation: op,
            target: Some(target),
            ..Default::default()
        },
    )
}

/// 機能 `id` の出力。
fn outputs(doc: &mut kmb_core::Document, k: &TestKernel, id: FeatureId) -> kmb_core::Outputs {
    let report = doc.recompute(&registry(), &**k);
    match report.feature(id).cloned() {
        Some(FeatureStatus::Ok { outputs, .. }) => outputs,
        s => panic!("{id}: {s:?}"),
    }
}

/// 予告の三角形が囲む体積(外向きの三角形なら正)。
fn enclosed(p: &ToolPreview) -> f64 {
    p.faces
        .iter()
        .map(|[a, b, c]| {
            let cr = [
                b[1] * c[2] - b[2] * c[1],
                b[2] * c[0] - b[0] * c[2],
                b[0] * c[1] - b[1] * c[0],
            ];
            (a[0] * cr[0] + a[1] * cr[1] + a[2] * cr[2]) / 6.0
        })
        .sum()
}

/// 20×20×10 の箱(辺の機能を入れ替える台)。
fn chamfer_bench() -> EdgeBench {
    let mut b = DocBuilder::new();
    let bl = block(&mut b, 0.0, 0.0, 20.0, 20.0, 10.0);
    EdgeBench::new(b.finish("面取り"), bl)
}

/// 台の箱の上面のまわりの 4 辺を、種類 `kind` で面取りするデータ。
fn top_chamfer(
    bench: &EdgeBench,
    row: serde_json::Value,
    kind: &str,
    extra: serde_json::Value,
) -> serde_json::Value {
    let bl = bench.target;
    let mut row = row;
    row["edges"] = json!([{"kind": "face", "feature": bl.0, "face": format!("f{}:end", bl.0)}]);
    let mut data = json!({"kind": kind, "rows": [row]});
    for (key, v) in extra.as_object().unwrap() {
        data[key] = v.clone();
    }
    data
}

/// 台の箱の上面のまわりの 4 辺を、種類 `kind` で面取りした時に削れる体積。
fn top_chamfer_removed(
    bench: &mut EdgeBench,
    row: serde_json::Value,
    kind: &str,
    extra: serde_json::Value,
) -> f64 {
    let data = top_chamfer(bench, row, kind, extra);
    4000.0 - bench.after(CHAMFER, data)
}

/// 4 辺の三角柱(面の上の幅 a・b)の体積から、角で 2 本が重なる分(a²·b/3 を 4 つ)を引く。
fn chamfer_volume(a: f64, b: f64) -> f64 {
    4.0 * a * b / 2.0 * 20.0 - 4.0 * a * a * b / 3.0
}

/// 箱の上面の面取りの種類を、面取りのデータだけ替えて確かめる(箱は 1 回だけ作る)。
/// - 2 つの距離: 距離 1 は上を向いた面の上、反転で入れ替わり、距離 2 が空なら等距離と同じ
/// - 距離と角度: 側面の上の幅は tan、45° は等距離と同じ、直角の辺で 90° 以上は理由を出す
#[test]
fn a_chamfer_takes_two_distances_or_a_distance_and_an_angle() {
    if TestKernel::new().is_truck() {
        kmb_contract::skip!("truck は距離の違う面取りで殻が閉じない(truck の中で panic する)");
    }
    let mut bench = chamfer_bench();
    // 距離 1(1 mm)は上を向いた上面の上、距離 2(2 mm)は側面の上
    let v = top_chamfer_removed(
        &mut bench,
        json!({"distance": "1 mm", "distance2": "2 mm"}),
        "two_distances",
        json!({}),
    );
    assert!((v - chamfer_volume(1.0, 2.0)).abs() < 0.3, "{v}");
    // 反転すると上面に 2 mm、側面に 1 mm
    let v = top_chamfer_removed(
        &mut bench,
        json!({"distance": "1 mm", "distance2": "2 mm", "flip": true}),
        "two_distances",
        json!({}),
    );
    assert!((v - chamfer_volume(2.0, 1.0)).abs() < 0.3, "{v}");
    // 距離 2 を空にすると距離 1 と同じ(等距離と同じ形)
    let v = top_chamfer_removed(
        &mut bench,
        json!({"distance": "1 mm"}),
        "two_distances",
        json!({}),
    );
    assert!((v - chamfer_volume(1.0, 1.0)).abs() < 0.3, "{v}");

    // 上面の上に 1 mm、上面から 30°: 側面の上の幅は tan 30° = 0.577 mm(直角の辺)
    let v = top_chamfer_removed(
        &mut bench,
        json!({"distance": "1 mm", "angle": "30 deg"}),
        "distance_angle",
        json!({}),
    );
    let t = 30f64.to_radians().tan();
    assert!((v - chamfer_volume(1.0, t)).abs() < 0.3, "{v}");
    // 45° は等距離と同じ。カーネルの丸め・三角の角を頼んでも、幅の違う面取りは柱で削る
    let v = top_chamfer_removed(
        &mut bench,
        json!({"distance": "1 mm", "angle": "45 deg"}),
        "distance_angle",
        json!({"method": "blend", "corner": "patch"}),
    );
    assert!((v - chamfer_volume(1.0, 1.0)).abs() < 0.3, "{v}");
    // 直角の辺では 90° 以上にできない(理由を出す)
    let data = top_chamfer(
        &bench,
        json!({"distance": "1 mm", "angle": "95 deg"}),
        "distance_angle",
        json!({}),
    );
    let st = bench.status(CHAMFER, data);
    let FeatureStatus::Error { message } = &st else {
        panic!("{st:?}");
    };
    assert!(message.contains("角度"), "{message}");
}

#[test]
fn a_linear_pattern_makes_a_grid_with_a_second_direction() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!(
            "truck は同じ平面で接する形のブーリアンに失敗する(箱と柱の下の面が同じ平面)"
        );
    }
    let make = |extra: serde_json::Value| {
        let mut b = DocBuilder::new();
        let bl = block(&mut b, 0.0, 0.0, 20.0, 20.0, 10.0);
        let bs = boss(&mut b, bl, Operation::Join, [1.0, 1.0, 2.0, 2.0, 12.0]);
        let mut data = json!({"feature": bs.0, "spacing": "5 mm", "count": "3"});
        for (key, v) in extra.as_object().unwrap() {
            data[key] = v.clone();
        }
        b.feature(LINEAR_PATTERN, "並び", data);
        b.finish("格子")
    };
    // 方向 2 の既定は 1 個(1 方向と同じ)
    let m = final_mass(&mut make(json!({})), &k);
    assert!(close(m.volume, 4000.0 + 3.0 * 8.0, 1e-6), "{}", m.volume);
    // 個数 2 を 2 にすると格子。向き 2 を省くと、向き(X)に直角な Y
    let m = final_mass(&mut make(json!({"spacing2": "7 mm", "count2": "2"})), &k);
    assert!(close(m.volume, 4000.0 + 6.0 * 8.0, 1e-6), "{}", m.volume);
    // 方向 2 を原点の X 軸(方向 1 と平行)にすると、理由つきで止まる
    let mut doc = make(json!({"count2": "2", "direction2_ref": {"kind": "origin_x"}}));
    let report = doc.recompute(&registry(), &*k);
    assert!(
        report.features.iter().any(
            |f| matches!(&f.status, FeatureStatus::Error { message } if message.contains("平行"))
        ),
        "{:?}",
        report.features.last()
    );
    // 保存の形: 前のデータは方向 2 を既定で読み、間隔 2 と個数 2 は書く(欄が空に見えない)
    let old = json!({"feature": 3, "spacing": "5 mm", "count": "3"});
    let p: LinearPattern = serde_json::from_value(old.clone()).unwrap();
    let v = serde_json::to_value(&p).unwrap();
    let now = json!({"feature": 3, "direction": [1.0, 0.0, 0.0], "spacing": "5 mm", "count": "3", "spacing2": "10 mm", "count2": "1"});
    assert_eq!(v, now);
    let d = registry().descriptor(LINEAR_PATTERN).unwrap();
    let mut up = old;
    up["direction"] = json!([1.0, 0.0, 0.0]);
    assert_eq!(d.upgraded(&up), now);
}

#[test]
fn pattern_copies_that_miss_the_body_are_counted() {
    // 20 mm の箱に 5 mm おきに 6 個の穴: 後ろの 2 個(x = 21・26)は箱の外で何も削らない
    let k = TestKernel::new();
    let make = |count: &str| {
        let mut b = DocBuilder::new();
        let bl = block(&mut b, 0.0, 0.0, 20.0, 20.0, 10.0);
        let cut = boss(&mut b, bl, Operation::Cut, [1.0, 1.0, 2.0, 2.0, 12.0]);
        let p = b.feature(
            LINEAR_PATTERN,
            "並び",
            json!({"feature": cut.0, "spacing": "5 mm", "count": count}),
        );
        (b.finish("外れた写し"), p)
    };
    let (mut doc, p) = make("6");
    let text = format!("{:?}", outputs(&mut doc, &k, p));
    assert!(
        text.contains("2 個の写しが本体に当たっていません(切り取りの写し 5 個のうち"),
        "{text}"
    );
    // みな当たっていれば注意は出ない
    let (mut doc, p) = make("4");
    let text = format!("{:?}", outputs(&mut doc, &k, p));
    assert!(!text.contains("当たっていません"), "{text}");
}

#[test]
fn bodies_can_be_patterned_and_mirrored_as_they_are() {
    let k = TestKernel::new();
    // 10×10×10 の箱を X に 15 mm おきに 3 つ(離れた 3 つの塊の 1 つのボディ)
    let mut b = DocBuilder::new();
    let bl = block(&mut b, 0.0, 0.0, 10.0, 10.0, 10.0);
    b.feature(
        LINEAR_PATTERN,
        "並び",
        LinearPattern {
            kind: kmb_cad_pattern::PatternKind::Bodies,
            bodies: vec![bl],
            spacing: "15 mm".into(),
            count: "3".into(),
            ..Default::default()
        },
    );
    let m = final_mass(&mut b.finish("ボディの並び"), &k);
    assert!(close(m.volume, 3000.0, 1e-6), "{}", m.volume);
    assert!(close(m.max[0], 40.0, 1e-6), "{:?}", m.max);
    // 原点の YZ 平面でボディを写す(裏返す写し)
    let mut b = DocBuilder::new();
    let bl = block(&mut b, 2.0, 0.0, 10.0, 10.0, 10.0);
    b.feature(
        MIRROR,
        "写し",
        json!({"kind": "bodies", "bodies": [bl.0], "plane_ref": {"kind": "origin_yz"}}),
    );
    let m = final_mass(&mut b.finish("ボディのミラー"), &k);
    assert!(close(m.volume, 2000.0, 1e-6), "{}", m.volume);
    assert!(close(m.min[0], -12.0, 1e-6), "{:?}", m.min);
    // 円形のパターンもボディで
    let mut b = DocBuilder::new();
    let bl = block(&mut b, 10.0, -2.0, 4.0, 4.0, 3.0);
    b.feature(
        CIRCULAR_PATTERN,
        "輪",
        json!({"kind": "bodies", "bodies": [bl.0], "axis_ref": {"kind": "origin_z"}, "count": "4"}),
    );
    let m = final_mass(&mut b.finish("ボディの輪"), &k);
    assert!(close(m.volume, 4.0 * 48.0, 1e-6), "{}", m.volume);
}

#[test]
fn a_linear_pattern_can_use_the_whole_width_and_center_on_the_original() {
    use kmb_cad_pattern::DistanceType;
    let k = TestKernel::new();
    // 10×10×10 の箱(X 0〜10)を X に並べる。(決め方, 距離, 個数, 対称) → (X の最小, 最大, 体積)
    for (kind, dist, n, sym, want) in [
        // 全体の幅 30 に 3 つ: 間隔 15
        (
            DistanceType::Extent,
            "30 mm",
            "3",
            false,
            (0.0, 40.0, 3000.0),
        ),
        // 対称で片側 2 つ: 元の両側に 15 mm おき(-15, 0, 15)
        (
            DistanceType::Spacing,
            "15 mm",
            "2",
            true,
            (-15.0, 25.0, 3000.0),
        ),
        // 対称の全体の幅 30 は両端の間: -15〜15
        (
            DistanceType::Extent,
            "30 mm",
            "2",
            true,
            (-15.0, 25.0, 3000.0),
        ),
    ] {
        let mut b = DocBuilder::new();
        let bl = block(&mut b, 0.0, 0.0, 10.0, 10.0, 10.0);
        b.feature(
            LINEAR_PATTERN,
            "並び",
            LinearPattern {
                kind: kmb_cad_pattern::PatternKind::Bodies,
                bodies: vec![bl],
                spacing: dist.into(),
                count: n.into(),
                distance_type: kind,
                symmetric: sym,
                ..Default::default()
            },
        );
        let m = final_mass(&mut b.finish("幅と対称"), &k);
        let case = format!("{kind:?} {dist} {n} {sym}");
        assert!(close(m.min[0], want.0, 1e-6), "{case}: {:?}", m.min);
        assert!(close(m.max[0], want.1, 1e-6), "{case}: {:?}", m.max);
        assert!(close(m.volume, want.2, 1e-6), "{case}: {}", m.volume);
    }
    // 方向 2 も対称にできる(Y に -12, 0, 12)
    let mut b = DocBuilder::new();
    let bl = block(&mut b, 0.0, 0.0, 10.0, 10.0, 10.0);
    b.feature(
        LINEAR_PATTERN,
        "並び",
        LinearPattern {
            kind: kmb_cad_pattern::PatternKind::Bodies,
            bodies: vec![bl],
            count: "1".into(),
            spacing2: "12 mm".into(),
            count2: "2".into(),
            symmetric2: true,
            ..Default::default()
        },
    );
    let m = final_mass(&mut b.finish("方向 2 の対称"), &k);
    assert!(
        close(m.min[1], -12.0, 1e-6) && close(m.max[1], 22.0, 1e-6),
        "{:?} {:?}",
        m.min,
        m.max
    );
    assert!(close(m.volume, 3000.0, 1e-6), "{}", m.volume);
    // 保存の形: 決め方が間隔で対称でなければ書かない(前のデータと同じ)
    let v = serde_json::to_value(LinearPattern::default()).unwrap();
    assert!(
        v.get("distance_type").is_none() && v.get("symmetric").is_none(),
        "{v}"
    );
}

#[test]
fn the_mirror_plane_can_be_an_origin_plane_or_a_flat_face() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!(
            "truck は同じ平面で接する形のブーリアンに失敗する(箱と柱の下の面が同じ平面)"
        );
    }
    let mut b = DocBuilder::new();
    let bl = block(&mut b, 0.0, 0.0, 20.0, 20.0, 10.0);
    let bs = boss(&mut b, bl, Operation::Join, [2.0, 2.0, 3.0, 3.0, 12.0]);
    let mi = b.feature(
        MIRROR,
        "写し",
        json!({"feature": bs.0, "plane_ref": {"kind": "origin_yz"}}),
    );
    let mut doc = b.finish("ミラー");
    // 原点の YZ 平面: 柱の写しは x -5〜-2(箱から離れた柱)
    let m = final_mass(&mut doc, &k);
    assert!(close(m.volume, 4000.0 + 18.0 + 108.0, 1e-6), "{}", m.volume);
    assert!(close(m.min[0], -5.0, 1e-6), "{:?}", m.min);
    // 箱の x = 20 の側面: 写しは x 35〜38
    let out = outputs(&mut doc, &k, bl);
    let kmb_core::Value::Shape(h) = &out["body"] else {
        panic!()
    };
    let side = k
        .faces(*h)
        .unwrap()
        .into_iter()
        .find(|f| f.planar.is_some_and(|p| p.normal[0] > 0.99))
        .unwrap()
        .name;
    doc.apply(kmb_core::Transaction::new(
        "平面",
        vec![kmb_core::Op::SetFeatureData {
            id: mi,
            data: json!({"feature": bs.0, "plane_ref": {"kind": "face", "feature": bl.0, "face": side}}),
        }],
    ))
    .unwrap();
    let m = final_mass(&mut doc, &k);
    assert!(close(m.max[0], 38.0, 1e-6), "{:?}", m.max);
    // 面をまだ選んでいない時は、選び方を書いて止まる
    doc.apply(kmb_core::Transaction::new(
        "平面",
        vec![kmb_core::Op::SetFeatureData {
            id: mi,
            data: json!({"feature": bs.0, "plane_ref": {"kind": "face"}}),
        }],
    ))
    .unwrap();
    let report = doc.recompute(&registry(), &*k);
    let Some(FeatureStatus::Error { message }) = report.feature(mi) else {
        panic!("{:?}", report.feature(mi));
    };
    assert!(message.contains("対称の平面"), "{message}");
}

#[test]
fn patterned_cuts_and_shells_preview_what_they_remove() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!(
            "truck は同じ平面で接する形のブーリアンに失敗する(箱と柱の下の面が同じ平面)。後半のシェルも truck は作れない"
        );
    }
    // 2×2 の貫通の切り取りを 3 つ並べる: 写しの 2 つの道具が予告に出る
    let mut b = DocBuilder::new();
    let bl = block(&mut b, 0.0, 0.0, 20.0, 20.0, 10.0);
    let cut = boss(&mut b, bl, Operation::Cut, [2.0, 2.0, 2.0, 2.0, 10.0]);
    let pat = b.feature(
        LINEAR_PATTERN,
        "並び",
        json!({"feature": cut.0, "spacing": "5 mm", "count": "3"}),
    );
    let mut doc = b.finish("切り取りの並び");
    let out = outputs(&mut doc, &k, pat);
    let p = <ToolPreview as kmb_core::Payload>::from_value(&out[kmb_core::TOOL_PREVIEW]).unwrap();
    assert_eq!(p.op, kmb_core::ToolOp::Cut);
    let v = enclosed(&p);
    // 道具は面と重ならないよう少し延ばすので、2 × 2 × 10 × 2 よりわずかに大きい
    assert!(v > 79.0 && v < 90.0, "{v}");
    // 結合のパターンは予告を出さない(結果の形が予告になる)
    let mut b = DocBuilder::new();
    let bl = block(&mut b, 0.0, 0.0, 20.0, 20.0, 10.0);
    let bs = boss(&mut b, bl, Operation::Join, [2.0, 2.0, 2.0, 2.0, 12.0]);
    let pat = b.feature(
        LINEAR_PATTERN,
        "並び",
        json!({"feature": bs.0, "spacing": "5 mm", "count": "3"}),
    );
    let mut doc = b.finish("結合の並び");
    assert!(!outputs(&mut doc, &k, pat).contains_key(kmb_core::TOOL_PREVIEW));

    // 上を開けたシェル: 削る所は内 16×16×8(x・y 2〜18、z 2〜10)。予告は内側の面を、削る所の
    // 外向きにした物(底の面は下向き)
    let mut b = DocBuilder::new();
    let bl = block(&mut b, 0.0, 0.0, 20.0, 20.0, 10.0);
    let sh = b.feature(
        SHELL,
        "シェル",
        Shell {
            faces: vec![Ref::Face {
                feature: bl,
                face: format!("f{}:end", bl.0),
            }],
            thickness: "2 mm".into(),
            direction: ShellDirection::Inside,
            tangent_chain: false,
        },
    );
    let mut doc = b.finish("シェル");
    let out = outputs(&mut doc, &k, sh);
    let p = <ToolPreview as kmb_core::Payload>::from_value(&out[kmb_core::TOOL_PREVIEW]).unwrap();
    assert_eq!(p.op, kmb_core::ToolOp::Cut);
    assert!(!p.faces.is_empty());
    for t in &p.faces {
        for q in t {
            assert!(
                (2.0 - 1e-6..=18.0 + 1e-6).contains(&q[0])
                    && (2.0 - 1e-6..=18.0 + 1e-6).contains(&q[1])
                    && (2.0 - 1e-6..=10.0 + 1e-6).contains(&q[2]),
                "{q:?}"
            );
        }
    }
    let floor: Vec<_> = p
        .faces
        .iter()
        .filter(|t| t.iter().all(|q| (q[2] - 2.0).abs() < 1e-6))
        .collect();
    assert!(!floor.is_empty());
    for [a, b, c] in floor {
        let nz = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        assert!(nz < 0.0, "底の面は下向き: {a:?} {b:?} {c:?}");
    }
}

/// パターンとミラーをボディを並べる形で始める時(1 つの登録簿で 3 種類を回す)。
/// - ボディを選んで始めると、ボディのパターンになる
/// - 道具の作り方を持つ機能が見えていない(最後がフィレット・面取り)時も、ボディを並べる形で
///   始められる。機能の候補があれば今までどおり機能を並べる
#[test]
fn a_pattern_starts_on_a_picked_body_or_when_no_feature_can_be_remade() {
    let reg = registry();
    // 見えているボディは 7(作り方は持たない)。機能の候補は無い
    let candidates = |slot: &str| if slot == "body" { vec![7] } else { vec![] };
    for t in [LINEAR_PATTERN, CIRCULAR_PATTERN, MIRROR] {
        let d = reg.descriptor(t).unwrap();
        // ボディを選んで始める
        let (data, _, _) = d
            .instantiate_with(&|_| vec![], &[Ref::Body(FeatureId(3))], &|_| false)
            .unwrap();
        assert_eq!(data["kind"], json!("bodies"), "{t}: {data}");
        assert_eq!(data["bodies"], json!(3), "{t}: {data}");
        // 何も選ばず、機能の候補が無い
        let (data, _, auto) = d.instantiate_with(&candidates, &[], &|_| false).unwrap();
        assert_eq!(data["kind"], json!("bodies"), "{t}: {data}");
        assert_eq!(data["bodies"], json!(7), "{t}: {data}");
        assert_eq!(auto, vec!["bodies".to_string()], "{t}");
        // 機能の候補があれば、今までどおり機能を並べる
        let (data, _, _) = d.instantiate_with(&|_| vec![5], &[], &|_| false).unwrap();
        assert_ne!(data["kind"], json!("bodies"), "{t}: {data}");
    }
}

/// ボディのパターンの写しを、1 つずつ別のボディに分ける。分ける前の写しは見えず注意が出る。
/// 個数を増やした分も注意が出て、もう一度分けると分かれる。個数を減らすと、余った写しのボディの
/// 機能だけが止まり、注意が理由を言う。写しのボディの機能を消すと分ける前に戻る。
#[test]
fn pattern_copies_can_be_split_into_their_own_bodies() {
    use kmb_cad_pattern::{PATTERN_COPY, SPLIT_COPIES};
    use kmb_core::{Context, Extension, Outcome, visible_bodies};
    use std::sync::Arc;
    let k: Arc<dyn kmb_core::Kernel> = Arc::new(kmb_kernel_smlib::SmlibKernel::new());
    let reg = registry();
    let mut b = DocBuilder::new();
    let bl = block(&mut b, 0.0, 0.0, 10.0, 10.0, 10.0);
    let p = b.feature(
        LINEAR_PATTERN,
        "並び",
        json!({"kind": "bodies", "bodies": [bl.0], "spacing": "15 mm", "count": "3", "separate": true}),
    );
    let mut doc = b.finish("写しを分ける");
    let cx_run = |doc: &kmb_core::Document,
                  report: &kmb_core::RecomputeReport,
                  run: &mut dyn FnMut(&Context<'_>)| {
        let cx = Context {
            selected: Some(p),
            ..Context::new(doc.data(), report, &reg, &k)
        };
        run(&cx);
    };
    let notes = |doc: &kmb_core::Document, report: &kmb_core::RecomputeReport| {
        let mut n = Vec::new();
        cx_run(doc, report, &mut |cx| n = MirrorCommands.notes(cx, p));
        n.join("\n")
    };
    // 分ける: 写しのボディの機能を足すだけ(パターンは書き換えない)
    let split = |doc: &mut kmb_core::Document| {
        let report = doc.recompute(&reg, &*k);
        let mut got = None;
        cx_run(doc, &report, &mut |cx| {
            got = MirrorCommands.run(SPLIT_COPIES, cx)
        });
        let Some(Ok(Outcome::Change { label, edits })) = got else {
            panic!("写しを分けられない");
        };
        doc.apply_edits(&label, edits).unwrap();
        doc.recompute(&reg, &*k)
    };
    let starts = |doc: &kmb_core::Document, report: &kmb_core::RecomputeReport| {
        let mut v: Vec<f64> = visible_bodies(doc.data(), &reg, report)
            .into_iter()
            .map(|(_, h)| {
                let m = k.mass(h).unwrap();
                assert!(
                    close(m.volume, 1000.0, 1e-6),
                    "写しは 1 つずつ: {}",
                    m.volume
                );
                m.min[0]
            })
            .collect();
        v.sort_by(f64::total_cmp);
        v
    };
    // 分ける前: 元と写し 1 が見え、写し 2 は注意で知らせる
    let report = doc.recompute(&reg, &*k);
    assert_eq!(starts(&doc, &report), vec![0.0, 15.0]);
    assert!(
        notes(&doc, &report).contains("写し 2 はまだ"),
        "{}",
        notes(&doc, &report)
    );
    let before = doc.data().clone();
    let report = split(&mut doc);
    assert_eq!(starts(&doc, &report), vec![0.0, 15.0, 30.0]);
    assert_eq!(notes(&doc, &report), "");
    assert_eq!(
        doc.data().feature(p),
        before.feature(p),
        "パターンは書き換えない"
    );

    // 個数を 4 にすると、増えた写し 3 は注意が出て、もう一度分けると 4 つが別々になる
    let mut data = doc.data().feature(p).unwrap().data.clone();
    data["count"] = json!("4");
    doc.apply(set(p, data)).unwrap();
    let report = doc.recompute(&reg, &*k);
    assert!(notes(&doc, &report).contains("写し 3 はまだ"));
    let report = split(&mut doc);
    assert_eq!(starts(&doc, &report), vec![0.0, 15.0, 30.0, 45.0]);
    let copies: Vec<_> = doc
        .data()
        .features
        .iter()
        .filter(|f| f.type_id == PATTERN_COPY)
        .map(|f| f.id)
        .collect();
    assert_eq!(copies.len(), 2);

    // 個数を 2 に減らすと、写し 2・3 のボディの機能だけが止まり、注意が理由を言う
    let mut data = doc.data().feature(p).unwrap().data.clone();
    data["count"] = json!("2");
    doc.apply(set(p, data)).unwrap();
    let report = doc.recompute(&reg, &*k);
    let stopped: Vec<_> = doc
        .data()
        .features
        .iter()
        .filter(|f| matches!(report.feature(f.id), Some(FeatureStatus::Error { .. })))
        .map(|f| f.id)
        .collect();
    assert_eq!(stopped, copies);
    assert!(notes(&doc, &report).contains("写し 2・3 のボディの機能は止まって"));
    assert_eq!(starts(&doc, &report), vec![0.0, 15.0]);
}
