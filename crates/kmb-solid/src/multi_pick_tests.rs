//! パターン・ミラーの複数の機能と、ブーリアンの「ツールを維持」。

use super::tests::{TestKernel, assert_ok, close, final_mass, rect_sketch, registry};
use super::*;
use kmb_core::FeatureId;
use kmb_samples::sample::DocBuilder;
use kmb_sketch::SketchPlane;

fn extrude(
    profile: FeatureId,
    distance: &str,
    op: Operation,
    target: Option<FeatureId>,
) -> Extrude {
    Extrude {
        profile: Some(profile),
        regions: Vec::new(),
        distance: distance.into(),
        direction: Direction::Normal,
        distance2: None,
        extent: Default::default(),
        to_face: None,
        to_offset: None,
        operation: op,
        target,
        ..Default::default()
    }
}

/// 40×10×4 の板に、x ごとに 2×2 の柱(高さ 7 = 板から 3 出る)を足し、2×2 の貫通の角穴を
/// あける(柱を全部作ってから角穴)。最初の柱と最初の角穴を返す。
fn plate_boss_cut(b: &mut DocBuilder, xs: &[f64]) -> (FeatureId, FeatureId) {
    plate_bosses_cuts(b, xs, xs)
}

fn plate_bosses_cuts(b: &mut DocBuilder, bosses: &[f64], cuts: &[f64]) -> (FeatureId, FeatureId) {
    let s = rect_sketch(b, SketchPlane::Xy, 0.0, 0.0, 40.0, 10.0);
    let mut body = b.feature(EXTRUDE, "板", extrude(s, "4 mm", Operation::New, None));
    let mut first = Vec::new();
    for (y, op, d, xs) in [
        (1.0, Operation::Join, "7 mm", bosses),
        (6.0, Operation::Cut, "4 mm", cuts),
    ] {
        for (i, x) in xs.iter().enumerate() {
            let s = rect_sketch(b, SketchPlane::Xy, *x, y, 2.0, 2.0);
            body = b.feature(EXTRUDE, "形", extrude(s, d, op, Some(body)));
            if i == 0 {
                first.push(body);
            }
        }
    }
    (first[0], first[1])
}

/// 手で並べた形の体積(パターン・ミラーの答え)。
fn by_hand(xs: &[f64]) -> f64 {
    by_hand2(xs, xs)
}

/// 別の文書なので別のカーネルで測る(同じカーネルで別の文書を再計算すると、前の文書の控えの形が消える)。
fn by_hand2(bosses: &[f64], cuts: &[f64]) -> f64 {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    plate_bosses_cuts(&mut b, bosses, cuts);
    final_mass(&mut b.finish("手で並べる"), &k).volume
}

/// 板に柱 1 本と角穴 1 つの文書を 1 回だけ作り、最後のパターン・ミラーだけを入れ替えて確かめる。
/// - 前の機能(柱の後に角穴)を並べると、足すボディは角穴の後のボディで、角穴も残る
/// - 直線のパターンは並べる機能を全部くり返す
/// - ミラーも並べる機能を全部写す
#[test]
fn patterns_and_mirrors_repeat_every_listed_feature() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は同じ平面で接する形のブーリアンに失敗する(板と柱の下の面が同じ平面)");
    }
    let mut b = DocBuilder::new();
    let (boss, cut) = plate_boss_cut(&mut b, &[2.0]);
    let pattern = |feature: Vec<FeatureId>, body: Option<FeatureId>, count: &str| LinearPattern {
        feature,
        body,
        direction_ref: Default::default(),
        direction: [1.0, 0.0, 0.0],
        spacing: "10 mm".into(),
        count: count.into(),
        ..Default::default()
    };
    // 柱の後に角穴がある。柱だけを並べ、足すボディを角穴の後のボディにする
    let p = b.feature(LINEAR_PATTERN, "並び", pattern(vec![boss], Some(cut), "2"));
    let mut doc = b.finish("前の機能を並べる");
    // ボディは 1 つのまま(final_mass が確かめる)で、角穴も残る
    let m = final_mass(&mut doc, &k);
    let v = by_hand2(&[2.0, 12.0], &[2.0]);
    assert!(close(m.volume, v, 1e-4), "{} vs {v}", m.volume);

    // 2 つの機能を 3 つ並べる
    let data = serde_json::to_value(pattern(vec![boss, cut], None, "3")).unwrap();
    doc.apply(kmb_core::Transaction::new(
        "並び",
        vec![kmb_core::Op::SetFeatureData { id: p, data }],
    ))
    .unwrap();
    let m = final_mass(&mut doc, &k);
    let v = by_hand(&[2.0, 12.0, 22.0]);
    assert!(close(m.volume, v, 1e-4), "{} vs {v}", m.volume);

    // 2 つの機能を写す(パターンをミラーに入れ替える)
    let mirror = Mirror {
        feature: vec![boss, cut],
        body: None,
        origin: Some([20.0, 0.0, 0.0]),
        normal: [1.0, 0.0, 0.0],
        ..Default::default()
    };
    let id = doc.alloc_feature_id();
    doc.apply(kmb_core::Transaction::new(
        "写し",
        vec![
            kmb_core::Op::RemoveFeature { id: p },
            kmb_core::Op::InsertFeature {
                index: None,
                entry: kmb_core::FeatureEntry {
                    id,
                    type_id: MIRROR.into(),
                    name: "写し".into(),
                    data: serde_json::to_value(mirror).unwrap(),
                    suppressed: false,
                },
            },
        ],
    ))
    .unwrap();
    let m = final_mass(&mut doc, &k);
    let v = by_hand(&[2.0, 36.0]);
    assert!(close(m.volume, v, 1e-4), "{} vs {v}", m.volume);
    assert!(close(m.max[2], 7.0, 1e-6));
}

#[test]
fn one_feature_is_still_written_as_a_bare_number() {
    let m = Mirror {
        feature: vec![FeatureId(4)],
        body: None,
        origin: None,
        normal: [1.0, 0.0, 0.0],
        ..Default::default()
    };
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(v["feature"], serde_json::json!(4));
    let back: Mirror =
        serde_json::from_value(serde_json::json!({"feature": [4, 6], "normal": [1, 0, 0]}))
            .unwrap();
    assert_eq!(back.feature, vec![FeatureId(4), FeatureId(6)]);
}

#[test]
fn an_empty_feature_list_is_an_error() {
    let m = LinearPattern {
        feature: vec![],
        body: None,
        direction_ref: Default::default(),
        direction: [1.0, 0.0, 0.0],
        spacing: "10 mm".into(),
        count: "2".into(),
        ..Default::default()
    };
    use kmb_core::Feature;
    let e = m.inputs().unwrap_err();
    assert!(e.to_string().contains("並べる機能を選んでください"), "{e}");
}

/// 柱 2 本(対象と道具)と、もう 1 本の道具で、道具を残すブーリアン。
fn kept_tool(keep_tools: bool) -> (usize, f64) {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 10.0, 10.0);
    let a = b.feature(EXTRUDE, "台", extrude(s, "5 mm", Operation::New, None));
    let s = rect_sketch(&mut b, SketchPlane::Xy, 5.0, 0.0, 10.0, 10.0);
    let t = b.feature(EXTRUDE, "道具", extrude(s, "5 mm", Operation::New, None));
    b.feature(
        BOOLEAN,
        "切り取り",
        BooleanFeature {
            target: a,
            tool: vec![t],
            operation: Operation::Cut,
            keep_tools,
        },
    );
    let mut doc = b.finish("ツールを維持");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    assert_ok(&report);
    let bodies = visible_bodies(doc.data(), &reg, &report);
    let total = bodies.iter().map(|(_, h)| k.mass(*h).unwrap().volume).sum();
    (bodies.len(), total)
}

#[test]
fn a_boolean_can_keep_its_tools() {
    if TestKernel::new().is_truck() {
        kmb_contract::skip!("truck は同じ平面で接する形のブーリアンに失敗する(台と道具の上下の面が同じ高さ)");
    }
    // 道具を使い切る(前と同じ): 残るのは切った台だけ
    let (n, v) = kept_tool(false);
    assert_eq!(n, 1);
    assert!(close(v, 5.0 * 10.0 * 5.0, 1e-4), "{v}");
    // 道具を残す: 切った台と道具の 2 つ
    let (n, v) = kept_tool(true);
    assert_eq!(n, 2);
    assert!(close(v, 5.0 * 10.0 * 5.0 + 10.0 * 10.0 * 5.0, 1e-4), "{v}");
}

#[test]
fn keep_tools_is_left_out_of_old_data() {
    let f = BooleanFeature {
        target: FeatureId(1),
        tool: vec![FeatureId(2)],
        operation: Operation::Cut,
        keep_tools: false,
    };
    let v = serde_json::to_value(&f).unwrap();
    assert!(v.get("keep_tools").is_none(), "{v}");
}
