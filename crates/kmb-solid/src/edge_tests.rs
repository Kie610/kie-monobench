//! フィレットと面取り(kmb-cad-edge)の通しの試験。

use super::*;
use crate::tests::*;
use kmb_core::{Document, FeatureStatus, Op};
use kmb_samples::sample::{DocBuilder, SketchBuilder, reference_part};
use kmb_sketch::SketchPlane;

#[test]
fn fillets_round_the_outer_vertical_edges() {
    let p = reference_part().plate.0;
    let side = |i: u32| format!("f{p}:side:e{i}");
    let edges: Vec<[String; 2]> = [(5, 6), (6, 7), (7, 8), (5, 8)]
        .iter()
        .map(|(a, b)| [side(*a), side(*b)])
        .collect();
    let (before, after, _) = with_edge_feature(
        FILLET,
        serde_json::json!({"target": 0, "edges": edges, "radius": "2 mm"}),
    );
    let r: f64 = 2.0;
    let removed = 4.0 * r * r * (1.0 - std::f64::consts::PI / 4.0) * 10.0;
    assert!(
        (before - after - removed).abs() < 1.0,
        "{before} - {after} は {removed} のはず"
    );
}

#[test]
fn a_fillet_too_large_for_its_faces_tells_the_largest_that_fits() {
    let p = reference_part().plate.0;
    let side = |i: u32| format!("f{p}:side:e{i}");
    let edges: Vec<[String; 2]> = [(5, 6), (6, 7), (7, 8), (5, 8)]
        .iter()
        .map(|(a, b)| [side(*a), side(*b)])
        .collect();
    // 幅 40 mm の側面は両端の角の丸めで分け合うので、1 つの角に使えるのは 20 mm まで
    let st = edge_feature_status(
        FILLET,
        serde_json::json!({"target": 0, "edges": edges, "radius": "25 mm"}),
    );
    let FeatureStatus::Error { message } = st else {
        panic!("誤りになるはず: {st:?}")
    };
    assert!(message.contains("収まる最大は 19.99 mm"), "{message}");
}

#[test]
fn a_fillet_with_fit_lowers_only_the_edge_that_does_not_fit() {
    let q = reference_part().pocket.0;
    // ポケット(15 × 12 mm)の縦の内角 1 つ。狭い壁は 12 mm
    let edge = [format!("f{q}:side:e6"), format!("f{q}:side:e7")];
    let st = edge_feature_status(
        FILLET,
        serde_json::json!({"target": 0, "edges": [edge.clone()], "radius": "13 mm"}),
    );
    let FeatureStatus::Error { message } = st else {
        panic!("誤りになるはず: {st:?}")
    };
    assert!(message.contains("収まる最大は 11.99 mm"), "{message}");

    let (before, after, mut doc) = with_edge_feature(
        FILLET,
        serde_json::json!({"target": 0, "edges": [edge], "radius": "13 mm", "fit": true}),
    );
    let r: f64 = 11.99;
    let added = r * r * (1.0 - std::f64::consts::PI / 4.0) * 4.0;
    assert!(
        (after - before - added).abs() < 0.5,
        "{after} - {before} は {added} のはず"
    );
    let k = TestKernel::new();
    let report = doc.recompute(&registry(), &*k);
    let last = report.features.last().unwrap();
    let FeatureStatus::Ok { outputs, .. } = &last.status else {
        panic!("{:?}", last.status)
    };
    let w = kmb_core::warnings_of(outputs);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("13 mm から 11.99 mm"), "{w:?}");
}

#[test]
fn a_fillet_that_fits_keeps_its_radius_and_gives_no_warning() {
    let q = reference_part().pocket.0;
    let edge = [format!("f{q}:side:e6"), format!("f{q}:side:e7")];
    let (_, _, mut doc) = with_edge_feature(
        FILLET,
        serde_json::json!({"target": 0, "edges": [edge], "radius": "2 mm", "fit": true}),
    );
    let k = TestKernel::new();
    let report = doc.recompute(&registry(), &*k);
    let last = report.features.last().unwrap();
    let FeatureStatus::Ok { outputs, .. } = &last.status else {
        panic!("{:?}", last.status)
    };
    assert!(kmb_core::warnings_of(outputs).is_empty());
}

#[test]
fn a_chamfer_on_the_top_face_breaks_its_outer_edges() {
    let p = reference_part().plate.0;
    let (before, after, _) = with_edge_feature(
        CHAMFER,
        serde_json::json!({"target": 0, "faces": [format!("f{p}:end")], "distance": "0.5 mm"}),
    );
    // 4 辺の三角柱(0.5 × 0.5 / 2 × 周長)から、角で重なる分(d³/3 × 4)を引く
    let d: f64 = 0.5;
    let mut removed = d * d / 2.0 * 200.0 - 4.0 * d * d * d / 3.0;
    if !TestKernel::new().is_truck() {
        // SMLib はブーリアンでできた辺も返すので、上面のまわりの出っ張った辺に
        // ポケットの縁(周長 2 × (15 + 12) mm)も入る(truck は落とす)。角の出入りは 1 mm³ 未満
        removed += d * d / 2.0 * 54.0;
    }
    assert!(
        (before - after - removed).abs() < 1.0,
        "{before} - {after} は {removed} のはず"
    );
}

#[test]
fn a_concave_corner_of_the_pocket_is_filled_with_a_round() {
    let q = reference_part().pocket.0;
    let (before, after, _) = with_edge_feature(
        FILLET,
        serde_json::json!({"target": 0, "edges": [[format!("f{q}:side:e6"), format!("f{q}:side:e7")]], "radius": "2 mm"}),
    );
    // 深さ 4 mm の縦の内角を埋める分
    let r: f64 = 2.0;
    let added = r * r * (1.0 - std::f64::consts::PI / 4.0) * 4.0;
    assert!(
        (after - before - added).abs() < 0.05,
        "{after} - {before} は {added} のはず"
    );
}

#[test]
fn the_pocket_floor_edges_get_rounds_and_chamfers() {
    let q = reference_part().pocket.0;
    let edges: Vec<[String; 2]> = (6..=9)
        .map(|i| [format!("f{q}:start"), format!("f{q}:side:e{i}")])
        .collect();
    let r: f64 = 1.0;
    let (before, after, _) = with_edge_feature(
        FILLET,
        serde_json::json!({"target": 0, "edges": edges, "radius": "1 mm"}),
    );
    // 周長 2 × (15 + 12) mm の四分円の残り。角での重なりは 1 mm³ 未満
    let added = r * r * (1.0 - std::f64::consts::PI / 4.0) * 54.0;
    assert!(
        (after - before - added).abs() < 1.0,
        "{after} - {before} は {added} のはず"
    );
    let (before, after, _) = with_edge_feature(
        CHAMFER,
        serde_json::json!({"target": 0, "edges": edges, "distance": "1 mm"}),
    );
    // 角では隣の三角柱と重なる(d³/3 × 4)
    let added = r * r / 2.0 * 54.0 - 4.0 * r * r * r / 3.0;
    assert!(
        (after - before - added).abs() < 1.0,
        "{after} - {before} は {added} のはず"
    );
}

#[test]
fn edge_features_report_bad_references() {
    let k = TestKernel::new();
    let reg = registry();
    let (p, q) = (reference_part().plate.0, reference_part().pocket.0);
    for (data, want) in [
        (
            serde_json::json!({"target": 0, "edges": [["f999:end", "f999:start"]], "radius": "1 mm"}),
            "見つかりません",
        ),
        (
            serde_json::json!({"target": 0, "edges": [[format!("f{q}:side:e6"), format!("f{q}:side:e7")]], "radius": "-1 mm"}),
            "0 より大きく",
        ),
        (
            serde_json::json!({"target": 0, "faces": [format!("f{p}:end")], "radius": "-1 mm"}),
            "0 より大きく",
        ),
    ] {
        let mut part = reference_part();
        let mut data = data;
        data["target"] = serde_json::json!(part.hole_pattern.0);
        let id = part.doc.alloc_feature_id();
        part.doc
            .apply(kmb_core::Transaction::new(
                "辺",
                vec![Op::InsertFeature {
                    index: None,
                    entry: kmb_core::FeatureEntry {
                        id,
                        type_id: FILLET.into(),
                        name: "辺".into(),
                        data,
                        suppressed: false,
                        pin_bodies: false,
                    },
                }],
            ))
            .unwrap();
        let report = part.doc.recompute(&reg, &*k);
        match report.feature(id) {
            Some(FeatureStatus::Error { message }) => {
                assert!(message.contains(want), "{message}")
            }
            other => panic!("{want} の誤りのはず: {other:?}"),
        }
    }
}

/// 穴の近くの根元フィレットの見本: 板(x 0〜30、y 0〜20、厚さ 5)の上面にフィン(x 14〜16、
/// 高さ 10、y 2〜18)を立て、フィンの右の根元(x = 16 のへこんだ辺)を半径 2 で
/// 丸める。上面の穴(径 5、貫通)の中心は (16 + `gap`, 10)。`gap` = 4.5 で穴の縁が丸め面の
/// 端に接し、4.0 で丸め面に 0.5 重なる。穴をあけてから丸める。
fn fin_with_hole(gap: f64) -> Document {
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 30.0, 20.0);
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(s),
            regions: Vec::new(),
            distance: "5 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    );
    let top = SketchPlane::face(plate, format!("f{}:end", plate.0));
    let fs = rect_sketch(&mut b, top.clone(), 14.0, 2.0, 2.0, 16.0);
    let fin = b.feature(
        EXTRUDE,
        "フィン",
        Extrude {
            profile: Some(fs),
            regions: Vec::new(),
            distance: "10 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::Join,
            target: Some(plate),
            ..Default::default()
        },
    );
    let mut s = SketchBuilder::new(top);
    let o = s.fixed_point(0.0, 0.0);
    let c = s.point_from(
        o,
        &format!("{} mm", 16.0 + gap),
        "10 mm",
        [16.0 + gap, 10.0],
    );
    let hs = b.feature(kmb_sketch::SKETCH, "穴の位置", s.build());
    let hole = b.feature(
        HOLE,
        "穴",
        Hole {
            target: fin,
            sketch: hs,
            points: vec![kmb_core::PointId(c.0)],
            diameter: "5 mm".into(),
            depth: None,
            ..Hole::new(fin, hs)
        },
    );
    b.feature(
        FILLET,
        "根元",
        Fillet::one(
            hole,
            EdgeSelection {
                // フィンの右の面(rect_sketch の線は下 e6、右 e7、上 e8、左 e9)と板の上面
                edges: vec![[format!("f{}:end", plate.0), format!("f{}:side:e7", fin.0)]],
                faces: vec![],
                arcs: false,
            },
            "2 mm",
            false,
            Corner::Miter,
        ),
    );
    b.finish("フィンと穴")
}

/// [`fin_with_hole`] の理論の体積。丸めで足す分(断面 r²(1 − π/4) × 長さ 16)から、
/// 穴と重なる分を数値積分で引く。
fn fin_with_hole_volume(gap: f64) -> f64 {
    use std::f64::consts::PI;
    let (r, rh) = (2.0_f64, 2.5_f64);
    let cx = 16.0 + gap;
    let base = 30.0 * 20.0 * 5.0 + 2.0 * 16.0 * 10.0 - PI * rh * rh * 5.0;
    let added = r * r * (1.0 - PI / 4.0) * 16.0;
    // 丸めで埋める所の高さ(x = 16 で r、x = 18 で 0)
    let height = |x: f64| r - (r * r - (16.0 + r - x).powi(2)).max(0.0).sqrt();
    let n = 20000;
    let mut overlap = 0.0;
    for i in 0..n {
        let x = 16.0 + r * (i as f64 + 0.5) / n as f64;
        let w = rh * rh - (x - cx).powi(2);
        if w > 0.0 {
            overlap += 2.0 * w.sqrt() * height(x) * r / n as f64;
        }
    }
    base + added - overlap
}

#[test]
fn a_root_fillet_does_not_fill_a_hole_made_before_it() {
    let k = TestKernel::new();
    if k.is_truck() {
        // truck はブーリアンでできた辺(フィンの根元)を返さないので、この辺は丸められない
        kmb_contract::skip!("truck はブーリアンでできた辺を返さない");
    }
    // 穴の縁が丸めの端に接する(4.5)、0.5 重なる(4.0)、1.5 重なる(3.0)。
    // 穴を見ないと 3.0 で穴の上を 0.8 mm³ 埋める。残る差は、丸めの円を面へ寄せる分
    // (OFFSETS、約 -0.064)と、穴の縁の手前で止める分(OUTLINE_GROW)
    for gap in [4.5, 4.0, 3.0] {
        let mut doc = fin_with_hole(gap);
        let m = final_mass(&mut doc, &k);
        let want = fin_with_hole_volume(gap);
        assert!(
            (m.volume - want).abs() < 0.1,
            "穴の中心 16 + {gap}: 体積 {} 理論 {want}",
            m.volume
        );
    }
}

/// 箱(x 0〜30、y 0〜30、高さ 15)の上面の手前の辺(y = 0)に `first` を掛け、続けて
/// 上面の右の辺(x = 30)に `second` を掛けた文書。どちらも機能の種類とデータ(対象と辺は
/// ここで入れる)。
fn box_edge_then(
    first: (&str, serde_json::Value),
    second: Option<(&str, serde_json::Value)>,
) -> Document {
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 30.0, 30.0);
    let p = b.feature(
        EXTRUDE,
        "箱",
        Extrude {
            profile: Some(s),
            distance: "15 mm".into(),
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
    let (top, front, right) = (
        format!("f{}:end", p.0),
        format!("f{}:side:e6", p.0),
        format!("f{}:side:e7", p.0),
    );
    let (type_id, mut data) = first;
    data["target"] = serde_json::json!(p);
    data["edges"] = serde_json::json!([[top, front]]);
    let f = b.feature(type_id, "手前", data);
    if let Some((type_id, mut data)) = second {
        data["target"] = serde_json::json!(f);
        data["edges"] = serde_json::json!([[top, right]]);
        b.feature(type_id, "右", data);
    }
    b.finish("箱")
}

/// [`box_edge_then`] の 2 つ目の機能が削った体積と、その理論値。理論値は、右の辺の角の断面
/// (u = 30 − x、w = 15 − z)で、2 つ目の機能が削る w の上限 `cut(u)`(0 から削る)のうち、
/// 1 つ目の機能の後に材料が残る(`top(y)` は y での上面の下がり)所を数値積分する。
fn second_cut(
    first: (&str, serde_json::Value),
    second: (&str, serde_json::Value),
    top: impl Fn(f64) -> f64,
    cut: impl Fn(f64) -> f64,
    reach: f64,
) -> (f64, f64) {
    let k = TestKernel::new();
    let before = final_mass(&mut box_edge_then(first.clone(), None), &k).volume;
    let after = final_mass(&mut box_edge_then(first, Some(second)), &k).volume;
    let (nu, ny) = (2000, 3000);
    let mut want = 0.0;
    for j in 0..ny {
        let drop = top(30.0 * (j as f64 + 0.5) / ny as f64);
        for a in 0..nu {
            let u = reach * (a as f64 + 0.5) / nu as f64;
            want += (cut(u) - drop).max(0.0) * reach / nu as f64 * 30.0 / ny as f64;
        }
    }
    (before - after, want)
}

#[test]
fn a_chamfer_after_a_fillet_runs_through_the_round_end() {
    if TestKernel::new().is_truck() {
        // truck はこの形の足し引きの結果が実行ごとに変わる
        kmb_contract::skip!("truck はこの形の足し引きの結果が実行ごとに変わる");
    }
    let (r, d): (f64, f64) = (5.0, 2.0);
    let (removed, want) = second_cut(
        (FILLET, serde_json::json!({"radius": format!("{r} mm")})),
        (CHAMFER, serde_json::json!({"distance": format!("{d} mm")})),
        // 丸めで下がった上面(右の辺の上、手前の y < r)
        |y| {
            if y < r {
                r - (r * r - (r - y).powi(2)).sqrt()
            } else {
                0.0
            }
        },
        |u| d - u,
        d,
    );
    // 面取りが丸めの始まりから d 先(y = 3)で止まると、手前の 2.4 mm³ が残る
    assert!(
        (removed - want).abs() < 0.03,
        "面取りで削った {removed} 理論 {want}"
    );
}

#[test]
fn a_fillet_after_a_chamfer_runs_through_the_slope() {
    if TestKernel::new().is_truck() {
        // truck はこの形の足し引きの結果が実行ごとに変わる
        kmb_contract::skip!("truck はこの形の足し引きの結果が実行ごとに変わる");
    }
    let (d, r): (f64, f64) = (4.0, 3.0);
    let (removed, want) = second_cut(
        (CHAMFER, serde_json::json!({"distance": format!("{d} mm")})),
        (FILLET, serde_json::json!({"radius": format!("{r} mm")})),
        // 面取りで下がった上面(手前の y < d は 45° の斜面)
        |y| (d - y).max(0.0),
        // 丸めの円は面からわずか(OFFSETS の最初の値 0.001)外へずらしてある
        |u| {
            let c = r - 0.001;
            (c - (r * r - (c - u).powi(2)).max(0.0).sqrt()).max(0.0)
        },
        r,
    );
    // 丸めが斜面の始まりから 1 mm 先(y = 3)で止まると、斜面の上に 0.23 mm³ が残る
    assert!(
        (removed - want).abs() < 0.01,
        "丸めで削った {removed} 理論 {want}"
    );
}

/// 箱(x 0〜30、y 0〜30、高さ 15)の角 (30, 0, 15) に集まる 3 本の辺(上面の手前、上面の右、
/// 手前の右の縦)に、角の形 `corner` で機能 `type_id`(データ `data`)を掛けて削った体積。
fn box_corner_removed(type_id: &str, mut data: serde_json::Value, corner: &str) -> f64 {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 30.0, 30.0);
    let p = b.feature(
        EXTRUDE,
        "箱",
        Extrude {
            profile: Some(s),
            distance: "15 mm".into(),
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
    let f = |n: &str| format!("f{}:{n}", p.0);
    data["target"] = serde_json::json!(p);
    data["edges"] = serde_json::json!([
        [f("end"), f("side:e6")],
        [f("end"), f("side:e7")],
        [f("side:e6"), f("side:e7")]
    ]);
    data["corner"] = serde_json::json!(corner);
    b.feature(type_id, "角", data);
    let mut doc = b.finish("箱の角");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    assert_ok(&report);
    let FeatureStatus::Ok { outputs, .. } = &report.features.last().unwrap().status else {
        unreachable!()
    };
    assert!(
        kmb_core::warnings_of(outputs).is_empty(),
        "{:?}",
        kmb_core::warnings_of(outputs)
    );
    30.0 * 30.0 * 15.0 - final_mass(&mut doc, &k).volume
}

#[test]
fn a_fillet_corner_is_a_ball_or_the_rounds_meeting() {
    let k = TestKernel::new();
    if k.is_truck() {
        // 角の形は SMLib だけで作る(truck の球の足し引きは確かめていない)
        kmb_contract::skip!("truck の球の足し引きは確かめていない");
    }
    use std::f64::consts::PI;
    let r: f64 = 4.0;
    let edges = r * r * (1.0 - PI / 4.0) * ((30.0 - r) * 2.0 + (15.0 - r));
    let data = serde_json::json!({"radius": format!("{r} mm")});
    // 交わり: 角の立方体 r³ のうち、3 本の円柱の重なり((2 − √2) r³)を残す
    // 丸めの円を面から外へずらした分(OFFSETS)で、どちらも理論より約 0.4 mm³ 少ない
    let miter = box_corner_removed(FILLET, data.clone(), "miter");
    let want = edges + r.powi(3) * (2.0_f64.sqrt() - 1.0);
    assert!((miter - want).abs() < 0.6, "交わり {miter} 理論 {want}");
    // 球: 角の立方体のうち、球の 8 分の 1 を残す
    let ball = box_corner_removed(FILLET, data, "ball");
    let want = edges + r.powi(3) * (1.0 - PI / 6.0);
    assert!((ball - want).abs() < 0.8, "球 {ball} 理論 {want}");
    // 2 つの差は、球のほうが (2 − π/6 − √2) r³ = 3.98 mm³ 多く削る。球は円筒にちょうど接しない
    // よう少し大きくしてある(BALL_GROW と辺ごとの差で約 0.007 mm、球の面 8 分の 1 で約 0.2 mm³)
    let diff = r.powi(3) * (2.0 - PI / 6.0 - 2.0_f64.sqrt());
    assert!(
        ((ball - miter) - diff).abs() < 0.3,
        "差 {} 理論 {diff}",
        ball - miter
    );
}

/// 箱の角の 3 本の面取り(距離 `d`)で削る体積の理論値。`patch` なら角に三角の面
/// (3 つの面それぞれで、面取りの境の線が交わる点を通る平面。u の和が 2d)を置く。
/// u は角からの、3 つの面までの距離。
fn box_corner_chamfer(d: f64, patch: bool) -> f64 {
    let (reach, n) = (3.0 * d, 1500);
    let cell = reach / n as f64;
    let mut v = d * d / 2.0 * ((30.0 - reach) * 2.0 + (15.0 - reach));
    for i in 0..n {
        for j in 0..n {
            let (ux, uy) = (cell * (i as f64 + 0.5), cell * (j as f64 + 0.5));
            // 縦の辺の面取り(ux + uy < d)は高さの全部を削る。ほかは uz の上限
            let top = if ux + uy < d {
                reach
            } else {
                let mut t = (d - ux).max(d - uy);
                if patch {
                    t = t.max(2.0 * d - ux - uy);
                }
                t.clamp(0.0, reach)
            };
            v += top * cell * cell;
        }
    }
    v
}

#[test]
fn a_chamfer_corner_is_a_triangle_or_the_chamfers_meeting() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck の角の形は確かめていない");
    }
    let d = 3.0;
    let data = serde_json::json!({"distance": format!("{d} mm")});
    // 辺ごとに距離を 0.001 mm ずつ変えた分で、どちらも理論より約 0.2 mm³ 多い
    let miter = box_corner_removed(CHAMFER, data.clone(), "miter");
    let want_miter = box_corner_chamfer(d, false);
    assert!(
        (miter - want_miter).abs() < 0.3,
        "交わり {miter} 理論 {want_miter}"
    );
    let patch = box_corner_removed(CHAMFER, data, "patch");
    let want = box_corner_chamfer(d, true);
    assert!((patch - want).abs() < 0.3, "三角の面 {patch} 理論 {want}");
    let diff = want - want_miter;
    assert!(
        ((patch - miter) - diff).abs() < 0.05,
        "差 {} 理論 {diff}",
        patch - miter
    );
}

#[test]
fn a_corner_shape_of_the_other_feature_is_an_error() {
    let st = edge_feature_status(
        FILLET,
        serde_json::json!({"target": 0, "faces": ["f2:end"], "radius": "1 mm", "corner": "patch"}),
    );
    assert!(matches!(st, FeatureStatus::Error { .. }), "{st:?}");
}

/// 丸めで削れた体積(丸める前の前方後円墳との差)。
fn keyhole_removed(w: f64, edges: &[[&str; 2]], k: &TestKernel) -> f64 {
    final_mass(&mut keyhole(w, &[]), k).volume - final_mass(&mut keyhole(w, edges), k).volume
}

/// 半径 2 の凸な丸めが 1 mm の長さあたり削る断面積(角 − 円)。
const ROUND2: f64 = 4.0 - std::f64::consts::PI;

#[test]
fn the_top_edge_of_a_cylinder_can_be_rounded_and_chamfered() {
    let k = TestKernel::new();
    if k.is_truck() {
        // truck は円の辺を返さない
        kmb_contract::skip!("truck は円の辺を返さない");
    }
    let reg = registry();
    for round in [true, false] {
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
        let sel = EdgeSelection {
            edges: vec![],
            faces: vec![format!("f{}:end", cyl.0)],
            arcs: true,
        };
        if round {
            b.feature(
                FILLET,
                "丸め",
                Fillet::one(cyl, sel, "2 mm", false, Default::default()),
            );
        } else {
            b.feature(
                CHAMFER,
                "面取り",
                Chamfer::one(cyl, sel, "2 mm", false, Default::default()),
            );
        }
        let mut doc = b.finish("円柱");
        let removed = std::f64::consts::PI * 1000.0 - final_mass(&mut doc, &k).volume;
        // パップスの定理: 断面積 × 断面の重心が回る長さ
        let (area, centroid) = if round {
            (
                ROUND2,
                2.0 * (10.0 - 3.0 * std::f64::consts::PI) / (3.0 * ROUND2),
            )
        } else {
            (2.0, 2.0 / 3.0)
        };
        let expect = area * std::f64::consts::TAU * (10.0 - centroid);
        assert!(
            close(removed, expect, 0.01),
            "round={round} {removed} {expect}"
        );
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
    }
}

#[test]
fn a_keyhole_top_is_rounded_along_the_arc_and_the_straight_edges() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は円の辺を返さない");
    }
    let arc = ["f2:end", "f2:side:e2"];
    let (e6, e7, e8) = (
        ["f2:end", "f{b}:side:e6"],
        ["f2:end", "f{b}:side:e7"],
        ["f2:end", "f{b}:side:e8"],
    );
    let removed = |edges: &[[&str; 2]]| keyhole_removed(10.0, edges, &k);
    let arc_only = removed(&[arc]);
    let side = removed(&[e8]);
    let end = removed(&[e7]);
    // 円の辺は 300° の弧(継ぎ目の切り口で少し増える)、直線の辺は長さ 25 − √75
    let arc_len = (10.0 - 0.4467) * 300f64.to_radians();
    assert!(close(arc_only, ROUND2 * arc_len, 0.02), "{arc_only}");
    assert!(close(side, ROUND2 * (25.0 - 75f64.sqrt()), 0.02), "{side}");
    assert!(close(end, ROUND2 * 10.0, 0.02), "{end}");
    // 全部: 足し合わせから、直方体の 2 つの角で重なる分(r³(5/3 − π/2))を引く
    let all = removed(&[e6, e7, e8, arc]);
    let corner = 8.0 * (5.0 / 3.0 - std::f64::consts::FRAC_PI_2);
    let expect = arc_only + 2.0 * side + end - 2.0 * corner;
    assert!(close(all, expect, 0.002), "{all} {expect}");
    // 辺の順によらない(直線の辺を先に丸めても壊れた形にならない)
    let reordered = removed(&[arc, e8, e7, e6]);
    // 辺ごとに半径をわずかにずらす(0.001 mm × 順番)ので、順で少しだけ変わる
    assert!(close(reordered, all, 0.002), "{reordered} {all}");
    // 継ぎ目に溝を掘らない: 円と直線を一緒に丸めても、別々に削った量の和(重なりはほぼ無い)
    let pair = removed(&[arc, e8]);
    assert!(
        (pair - (arc_only + side)).abs() < 0.05,
        "{pair} {arc_only} {side}"
    );
}

#[test]
fn rounding_the_keyhole_top_face_reaches_the_arc_edge() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は円の辺を返さない");
    }
    let all = keyhole_removed(
        10.0,
        &[
            ["f2:end", "f2:side:e2"],
            ["f2:end", "f{b}:side:e6"],
            ["f2:end", "f{b}:side:e7"],
            ["f2:end", "f{b}:side:e8"],
        ],
        &k,
    );
    let before = final_mass(&mut keyhole(10.0, &[]), &k).volume;
    let mut doc = keyhole(10.0, &[]);
    let id = doc.alloc_feature_id();
    doc.apply(kmb_core::Transaction::new(
        "丸め",
        vec![Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: FILLET.into(),
                name: "丸め".into(),
                data: serde_json::json!({"target": 4, "faces": ["f2:end"], "arcs": true, "radius": "2 mm"}),
                suppressed: false, pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    // 面を選んで丸めると、直線の 3 本だけでなく円の辺も含めた 4 本を丸める
    let after = final_mass(&mut doc, &k).volume;
    assert!(
        close(before - after, all, 0.002),
        "{} {all}",
        before - after
    );
}

/// [`plate_with_hole`] の板(手前の縁 y = 0 から 2 mm の所に径 4 mm の貫通穴)の上面の長い辺
/// (`side`: 手前 e6、奥 e8)を、半径 `radius` で丸めた後の状態と体積。
fn fillet_beside_a_hole(side: u32, radius: &str, fit: bool) -> (FeatureStatus, Option<f64>) {
    let k = TestKernel::new();
    let mut doc = plate_with_hole(
        &[[15.0, 4.0]],
        |t, s, points| Hole {
            points,
            diameter: "4 mm".into(),
            ..Hole::new(t, s)
        },
        false,
    );
    let find = |doc: &Document, name: &str| {
        doc.data()
            .features
            .iter()
            .find(|f| f.name == name)
            .unwrap()
            .id
    };
    let (plate, hole) = (find(&doc, "板"), find(&doc, "穴"));
    let id = doc.alloc_feature_id();
    let edge = [
        format!("f{}:end", plate.0),
        format!("f{}:side:e{side}", plate.0),
    ];
    doc.apply(kmb_core::Transaction::new(
        "丸め",
        vec![Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: FILLET.into(),
                name: "丸め".into(),
                data: serde_json::json!({
                    "target": hole.0, "edges": [edge], "radius": radius, "fit": fit
                }),
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let report = doc.recompute(&registry(), &*k);
    let st = report.feature(id).unwrap().clone();
    let v = matches!(st, FeatureStatus::Ok { .. }).then(|| final_mass(&mut doc, &k).volume);
    (st, v)
}

#[test]
fn a_fillet_beside_a_hole_can_use_the_largest_radius_it_suggests() {
    // 穴の隣の丸めで、案内した最大の半径が通ること。JC は
    // 穴にかかる丸めも作れる(shadow)ので、案内する最大は側面の高さ 10 mm で決まり、穴の隣でも通る
    if TestKernel::new().is_truck() {
        // truck は丸めの柱と穴の円柱のブーリアンが閉じた殻にならない(実測)。既定の SMLib で確かめる
        kmb_contract::skip!("truck は丸めの柱と穴の円柱のブーリアンが閉じた殻にならない");
    }
    let (st, _) = fillet_beside_a_hole(6, "25 mm", false);
    let FeatureStatus::Error { message } = st else {
        panic!("誤りになるはず: {st:?}")
    };
    assert!(message.contains("収まる最大は 9.99 mm"), "{message}");
    for (radius, fit) in [
        ("9.99 mm", false),
        ("25 mm", true),
        ("2 mm", false),
        ("6 mm", false),
    ] {
        let (near, v_near) = fillet_beside_a_hole(6, radius, fit);
        let (far, v_far) = fillet_beside_a_hole(8, radius, fit);
        let (Some(v_near), Some(v_far)) = (v_near, v_far) else {
            panic!("{radius}: 穴の隣 {near:?}、穴のない側 {far:?}")
        };
        // 穴の隣では、丸めで削る所の一部が初めから穴なので、削る量は穴のない側より少ない
        // (多くても穴の体積まで)
        let hole = std::f64::consts::PI * 2.0 * 2.0 * 10.0;
        assert!(
            v_near >= v_far - 1e-3 && v_near <= v_far + hole,
            "{radius}: 穴の隣 {v_near}、穴のない側 {v_far}"
        );
    }
}

#[test]
fn a_root_fillet_beside_a_hole_can_use_the_largest_radius_it_suggests() {
    let k = TestKernel::new();
    if k.is_truck() {
        // truck はブーリアンでできた辺(フィンの根元)を返さないので、この辺は丸められない
        kmb_contract::skip!("truck はブーリアンでできた辺を返さない");
    }
    // フィンの根元を丸め、半径を下げた値(フィンの高さ 10 mm まで)が穴(縁がフィンから 0.5 mm)にかかっても通る
    let mut doc = fin_with_hole(3.0);
    let f = doc
        .data()
        .features
        .iter()
        .find(|f| f.name == "根元")
        .unwrap()
        .clone();
    let mut data = f.data.clone();
    data["rows"][0]["radius"] = serde_json::json!("25 mm");
    data["fit"] = serde_json::json!(true);
    doc.apply(kmb_core::Transaction::new(
        "半径",
        vec![Op::SetFeatureData { id: f.id, data }],
    ))
    .unwrap();
    let report = doc.recompute(&registry(), &*k);
    let FeatureStatus::Ok { outputs, .. } = report.feature(f.id).unwrap() else {
        panic!("{:?}", report.feature(f.id))
    };
    let w = kmb_core::warnings_of(outputs);
    assert!(w.len() == 1 && w[0].contains("25 mm から 9.99 mm"), "{w:?}");
}

/// 60×40×10 の板に、角を半径 4 で丸めた 30×20 の深さ 4 のポケット(線と弧の ID は 13〜20)。
/// 文書とカーネルは 1 回だけ作り、`swap_edge` で最後の辺の機能だけを入れ替える。
struct RoundedPocket {
    bench: EdgeBench,
    plate: kmb_core::FeatureId,
    pocket: kmb_core::FeatureId,
}

impl RoundedPocket {
    fn new() -> Self {
        let mut b = DocBuilder::new();
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        let o = s.fixed_point(0.0, 0.0);
        s.rectangle(o, "60 mm", "40 mm", [0.0, 0.0, 60.0, 40.0]);
        let sk = b.feature(kmb_sketch::SKETCH, "板の輪郭", s.build());
        let extrude = |profile, distance: &str, direction, operation, target| Extrude {
            profile: Some(profile),
            regions: Vec::new(),
            distance: distance.into(),
            direction,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation,
            target,
            ..Default::default()
        };
        let plate = b.feature(
            kmb_cad_extrude::EXTRUDE,
            "板",
            extrude(sk, "10 mm", Direction::Normal, Operation::New, None),
        );
        let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
        let ids = s.rounded_rectangle(15.0, 10.0, 30.0, 20.0, 4.0);
        assert_eq!(ids[0].0, 13);
        let sk = b.feature(kmb_sketch::SKETCH, "ポケットの輪郭", s.build());
        let pocket = b.feature(
            kmb_cad_extrude::EXTRUDE,
            "ポケット",
            extrude(sk, "4 mm", Direction::Reverse, Operation::Cut, Some(plate)),
        );
        let bench = EdgeBench::new(b.finish("角を丸めたポケット"), pocket);
        RoundedPocket {
            bench,
            plate,
            pocket,
        }
    }

    /// 最後の辺の機能を `edge`(対象と、行の辺の機能は自動で入れる)に入れ替え、後の体積を返す。
    fn swap_edge(&mut self, type_id: &str, mut edge: serde_json::Value) -> f64 {
        let (plate, pocket) = (self.plate.0.to_string(), self.pocket.0.to_string());
        for row in edge["rows"].as_array_mut().unwrap() {
            row["edges"]["feature"] = serde_json::json!(self.pocket.0);
            for f in row["edges"]["faces"].as_array_mut().unwrap() {
                let name = f.as_str().unwrap().replace("{plate}", &plate);
                *f = serde_json::json!(name.replace("{pocket}", &pocket));
            }
        }
        self.bench.after(type_id, edge)
    }
}

/// 1 行の辺の機能。`faces` の `{plate}`・`{pocket}` は機能の番号に置き換える。`method` は丸め方
/// (`cut` は前の文書と同じ柱で削る形、`blend` はカーネルの丸め)。
fn one_edge_row(key: &str, faces: [&str; 2], chain: bool, method: &str) -> serde_json::Value {
    serde_json::json!({
        "rows": [{
            "edges": {"kind": "edge", "feature": 0, "faces": faces},
            key: "1 mm",
        }],
        "chain": chain,
        "method": method,
    })
}

/// カーネルの丸めで作った時の、理論との差の許し。truck は柱で削る形で作るので、円の辺の面取りは
/// 理論から 1% ほどずれる
fn blend_tolerance(k: &TestKernel) -> f64 {
    if k.is_truck() { 0.6 } else { 0.01 }
}

/// ポケットの縁の直線の長さ(角の弧を除く)と、角の半径
const POCKET_STRAIGHT: f64 = 2.0 * (30.0 - 8.0) + 2.0 * (20.0 - 8.0);
const POCKET_CORNER: f64 = 4.0;

/// 角を丸めたポケットの縁(直線 4 本と四分円 4 本)を、1 本の辺を選ぶだけで「つながるエッジも入れる」で
/// 丸める・面取りする。削る体積は、直線は断面 × 長さ、円弧はパップスの定理(断面 × 重心の回る長さ)。
/// - 上の縁の丸めは、柱で削る形でもカーネルの丸めでも輪を一周し、入れなければ選んだ 1 本だけ
/// - へこんだ底の縁の丸めも選んだ 1 本からたどる(中身を足す。弧では中身が軸の側)
/// - 上の縁の面取りも同じく一周する(断面 d²/2、重心は角から d/3)
#[test]
fn chained_fillets_and_chamfers_run_round_the_whole_rounded_pocket_rim() {
    if TestKernel::new().is_truck() {
        kmb_contract::skip!("truck は角を丸めたポケットの縁の弧の辺を返さない");
    }
    use std::f64::consts::PI;
    let mut p = RoundedPocket::new();
    let before = p.bench.before;
    let r = 1.0_f64;
    let area = r * r * (1.0 - PI / 4.0);
    // 断面(角の正方形 − 四分円)の重心は角から d
    let d = r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);

    // 上の縁の丸め。つながる辺も: ポケットの角では中身が弧の外側
    let top = ["f{plate}:end", "f{pocket}:side:e13"];
    let want = area * POCKET_STRAIGHT + area * 2.0 * PI * (POCKET_CORNER + d);
    // 前の文書と同じ柱で削る形でも、カーネルの丸めでも
    for (method, tol) in [("cut", 0.1), ("blend", blend_tolerance(&p.bench.k))] {
        let after = p.swap_edge(FILLET, one_edge_row("radius", top, true, method));
        assert!(
            (before - after - want).abs() < tol,
            "{method} 輪: {} 理論 {want}",
            before - after
        );
    }
    // 入れなければ、選んだ 1 本(22 mm)だけ
    let after = p.swap_edge(FILLET, one_edge_row("radius", top, false, "cut"));
    let want = area * 22.0;
    assert!(
        (before - after - want).abs() < 0.1,
        "1 本: {} 理論 {want}",
        before - after
    );

    // へこんだ底の縁
    let floor = ["f{pocket}:start", "f{pocket}:side:e15"];
    let after = p.swap_edge(FILLET, one_edge_row("radius", floor, true, "blend"));
    let want = area * POCKET_STRAIGHT + area * 2.0 * PI * (POCKET_CORNER - d);
    assert!(
        (after - before - want).abs() < blend_tolerance(&p.bench.k).max(0.1),
        "底: {} 理論 {want}",
        after - before
    );

    // 面取りも同じ。距離 d の面取りの断面は d²/2、重心は角から d/3。
    let top = ["f{plate}:end", "f{pocket}:side:e17"];
    let after = p.swap_edge(CHAMFER, one_edge_row("distance", top, true, "blend"));
    let dist = 1.0_f64;
    let area = dist * dist / 2.0;
    let want = area * POCKET_STRAIGHT + area * 2.0 * PI * (POCKET_CORNER + dist / 3.0);
    assert!(
        (before - after - want).abs() < blend_tolerance(&p.bench.k),
        "輪: {} 理論 {want}",
        before - after
    );
}

/// 接してつながる辺のたどりは、形の大きさによらない。角を丸めた
/// 長方形の柱の上の縁を、どの直線から始めても、1 mm・10 mm・1000 mm のどの大きさでも直線 4 本と弧 4 本の
/// 一周になる(端の点の重なりの許しが mm の固定値なので、大きな形でカーネルの誤差が許しを越えないか)。
#[test]
fn the_tangent_chain_runs_round_a_rounded_rim_at_any_size() {
    let k = TestKernel::new();
    if k.is_truck() {
        // truck は柱の角の四分円の辺を辺の一覧に返さない(上の縁は直線 4 本だけになる)
        kmb_contract::skip!("truck は角を丸めた柱の弧の辺を返さない");
    }
    let reg = registry();
    for size in [1.0_f64, 10.0, 1000.0] {
        let mut b = DocBuilder::new();
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        s.rounded_rectangle(0.0, 0.0, size * 1.5, size, size / 5.0);
        let sk = b.feature(kmb_sketch::SKETCH, "輪郭", s.build());
        b.feature(
            kmb_cad_extrude::EXTRUDE,
            "柱",
            Extrude {
                profile: Some(sk),
                distance: format!("{} mm", size / 2.0),
                ..Default::default()
            },
        );
        let mut doc = b.finish("柱");
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
        let bodies = visible_bodies(doc.data(), &reg, &report);
        let h = bodies[0].1;
        let mut all = k.edges(h).unwrap();
        all.extend(k.arc_edges(h).unwrap());
        all.extend(k.curve_edges(h).unwrap());
        let top: Vec<usize> = (0..all.len())
            .filter(|&i| all[i].a[2] > size / 4.0 && all[i].b[2] > size / 4.0)
            .collect();
        assert_eq!(top.len(), 8, "{size} mm: 上の縁の辺 {}", top.len());
        for &seed in top.iter().filter(|&&i| all[i].arc.is_none()) {
            let mut chain = kmb_cad::tangent_chain(&all, seed, kmb_cad::TANGENT_DEG);
            chain.sort();
            assert_eq!(chain, top, "{size} mm: 辺 {seed} から");
        }
    }
}

/// 項目の無い前の文書は「つながるエッジも入れる」をしない。画面と CLI から新しく作るものはする。
#[test]
fn old_fillets_read_without_chain_and_new_ones_have_it() {
    let f: kmb_cad_edge::Fillet = serde_json::from_value(
        serde_json::json!({"target": 1, "rows": [{"edges": [], "radius": "1 mm"}]}),
    )
    .unwrap();
    assert!(!f.chain);
    let old: kmb_cad_edge::Fillet = serde_json::from_value(
        serde_json::json!({"target": 1, "edges": [["a", "b"]], "radius": "1 mm"}),
    )
    .unwrap();
    assert!(!old.chain);
    let back = serde_json::to_value(&f).unwrap();
    assert!(back.get("chain").is_none(), "{back}");
    let reg = registry();
    for t in [FILLET, CHAMFER] {
        let d = reg.descriptor(t).unwrap();
        assert_eq!(d.create_defaults.as_ref().unwrap()["chain"], true, "{t}");
    }
}

/// ポケットの縁(へこんだ角で出会う辺)の面取りは、角に細い面を残さない。2 本の面取りは角で
/// 交わる線で終わる。柱を二等分の面の先まで残すと、その先の柱が隣の面取りの外を削り、
/// 幅 0.014 mm ほどの面が角ごとに残っていた(それを横切る押し出しで SMLib が立体を失った)。
#[test]
fn a_chamfer_on_the_pocket_rim_leaves_no_thin_face_at_its_corners() {
    let k = TestKernel::new();
    let reg = registry();
    let mut part = reference_part();
    let p = part.plate.0;
    let id = part.doc.alloc_feature_id();
    part.doc
        .apply(kmb_core::Transaction::new(
            "面取り",
            vec![Op::InsertFeature {
                index: None,
                entry: kmb_core::FeatureEntry {
                    id,
                    type_id: CHAMFER.into(),
                    name: "面取り".into(),
                    data: serde_json::json!({
                        "target": part.hole_pattern.0,
                        "faces": [format!("f{p}:end")],
                        "distance": "1 mm",
                    }),
                    suppressed: false,
                    pin_bodies: false,
                },
            }],
        ))
        .unwrap();
    let report = part.doc.recompute(&reg, &*k);
    assert_ok(&report);
    let body = visible_bodies(part.doc.data(), &reg, &report)[0].1;
    let thin: Vec<(String, f64)> = k
        .faces(body)
        .unwrap()
        .into_iter()
        .filter(|f| f.area < 0.1)
        .map(|f| (f.name, f.area))
        .collect();
    assert!(thin.is_empty(), "細い面が残った: {thin:?}");
}

/// 確かめること: 円筒どうしの交線に一定幅でない面取りを誤って作らず、
/// 切れた参照と区別して、使える面取りの種類を知らせる。
#[test]
fn uneven_curved_chamfers_explain_the_supported_alternative() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は円筒どうしの曲がった交線を列挙できない");
    }
    let reg = registry();
    let mut b = DocBuilder::new();
    let mut circle = |plane, x, y, diameter: &str, radius, target: Option<kmb_core::FeatureId>| {
        let mut s = SketchBuilder::new(plane);
        let center = s.fixed_point(x, y);
        s.circle(center, diameter, radius);
        let sk = b.feature(kmb_sketch::SKETCH, "円", s.build());
        b.feature(
            EXTRUDE,
            "円柱",
            Extrude {
                profile: Some(sk),
                distance: "40 mm".into(),
                start: Start::Offset,
                start_offset: Some("-20 mm".into()),
                operation: if target.is_some() {
                    Operation::Intersect
                } else {
                    Operation::New
                },
                target,
                ..Default::default()
            },
        )
    };
    let a = circle(SketchPlane::Xy, 0.0, 0.0, "20 mm", 10.0, None);
    let target = circle(SketchPlane::Zx, 0.0, 1.0, "14 mm", 7.0, Some(a));
    let mut doc = b.finish("曲線の縁");
    let report = doc.recompute(&reg, &*k);
    assert_ok(&report);
    let body = visible_bodies(doc.data(), &reg, &report)[0].1;
    let before_volume = k.volume(body).unwrap();
    let curves = k.curve_edges(body).unwrap();
    assert!(!curves.is_empty(), "円筒の交線が必要です");
    let edge = &curves[0];
    for kind in ["two_distances", "distance_angle", "equal"] {
        for flip in [false, true] {
            let mut attempt = Document::from_json(&doc.to_json()).unwrap();
            let id = attempt.alloc_feature_id();
            attempt.apply(kmb_core::Transaction::new("面取り", vec![Op::InsertFeature {
                index: None, entry: kmb_core::FeatureEntry {
                    id, type_id: CHAMFER.into(), name: "面取り".into(),
                    data: serde_json::json!({"target": target, "kind": kind, "method": "blend", "rows": [{
                        "edges": [{"kind": "edge", "feature": target, "faces": edge.faces}],
                        "distance": "0.1 mm", "distance2": "0.105 mm", "angle": "30 deg", "flip": flip,
                    }]}), suppressed: false, pin_bodies: false,
                }
            }])).unwrap();
            let r = attempt.recompute(&reg, &*k);
            if kind == "equal" {
                assert_ok(&r);
                let result = visible_bodies(attempt.data(), &reg, &r)[0].1;
                assert!(k.volume(result).unwrap() < before_volume);
                continue;
            }
            let FeatureStatus::Error { message } = &r.features.last().unwrap().status else {
                panic!("{r:?}")
            };
            assert!(
                message.contains("曲がった辺には"),
                "{kind} flip={flip}: {message}"
            );
            assert!(message.contains("等しい距離"), "{message}");
            assert!(!message.contains("参照が切れ"), "{message}");
        }
    }
}
