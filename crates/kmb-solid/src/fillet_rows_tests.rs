//! フィレット・面取りの行(1 つの機能に違う半径の組をいくつも持つ)。

use super::tests::{EdgeBench, with_edge_feature};
use super::*;
use kmb_core::{FeatureId, FeatureStatus};
use kmb_samples::sample::reference_part;

/// 基準部品の板の縦の辺(外周の角)。`(a, b)` は板の側面の番号の組。
fn corner(a: u32, b: u32) -> serde_json::Value {
    let p = reference_part().plate.0;
    serde_json::json!({"kind": "edge", "feature": 0, "faces": [format!("f{p}:side:e{a}"), format!("f{p}:side:e{b}")]})
}

/// 板の高さ 10 mm の縦の辺を半径 r で丸めて削れる体積。
fn rounded(r: f64) -> f64 {
    r * r * (1.0 - std::f64::consts::PI / 4.0) * 10.0
}

/// 基準部品の最後のフィレットの行だけを入れ替えて確かめる(部品は 1 回だけ作る)。
/// - 各行は自分の辺を自分の半径で丸める
/// - 2 つの行にある辺は、後の行の半径になる
/// - 前の形(edges と radius)と行の形で、同じ辺を同じ半径で丸めれば体積も同じ
/// - 誤りは何行目かを言う
#[test]
fn fillet_rows_round_their_own_edges_and_errors_name_the_row() {
    let mut bench = EdgeBench::reference();
    let before = bench.before;
    let after = bench.after(
        FILLET,
        serde_json::json!({"rows": [
            {"edges": [corner(5, 6), corner(7, 8)], "radius": "1 mm"},
            {"edges": [corner(6, 7), corner(5, 8)], "radius": "2 mm"},
        ]}),
    );
    let removed = 2.0 * rounded(1.0) + 2.0 * rounded(2.0);
    assert!(
        (before - after - removed).abs() < 0.5,
        "{before} - {after} は {removed} のはず"
    );

    // 2 つの行にある辺は後の行
    let after = bench.after(
        FILLET,
        serde_json::json!({"rows": [
            {"edges": [corner(5, 6), corner(7, 8)], "radius": "1 mm"},
            {"edges": corner(7, 8), "radius": "2 mm"},
        ]}),
    );
    let removed = rounded(1.0) + rounded(2.0);
    assert!(
        (before - after - removed).abs() < 0.5,
        "{before} - {after} は {removed} のはず"
    );

    // 前の形と同じ体積
    let p = reference_part().plate.0;
    let side = |i: u32| format!("f{p}:side:e{i}");
    let a0 = bench.after(
        FILLET,
        serde_json::json!({"edges": [[side(5), side(6)], [side(7), side(8)]], "radius": "2 mm"}),
    );
    let a1 = bench.after(
        FILLET,
        serde_json::json!({"rows": [{"edges": [corner(5, 6), corner(7, 8)], "radius": "2 mm"}]}),
    );
    // 同じ辺を同じ半径で丸める。体積は網から求めるので、Windows(MSVC)の SMLib では
    // 4e-5 ほど(相対 3e-6)ずれることがある。ほかの体積の試験と同じく相対で比べる
    let (v0, v1) = (before - a0, before - a1);
    assert!((v0 - v1).abs() <= 1e-4 * v0.abs(), "{v0} と {v1}");

    // 誤りは何行目か
    let status = bench.status(
        FILLET,
        serde_json::json!({"rows": [
            {"edges": corner(5, 6), "radius": "1 mm"},
            {"edges": corner(7, 8), "radius": "-1 mm"},
        ]}),
    );
    match status {
        FeatureStatus::Error { message } => {
            assert!(message.contains("選択セット 2: 半径は"), "{message}")
        }
        other => panic!("2 行目の誤りのはず: {other:?}"),
    }
}

#[test]
fn a_chamfer_also_takes_a_distance_per_row() {
    let (before, after, _) = with_edge_feature(
        CHAMFER,
        serde_json::json!({"target": 0, "rows": [
            {"edges": corner(5, 6), "distance": "1 mm"},
            {"edges": corner(7, 8), "distance": "2 mm"},
        ]}),
    );
    // 面取りは 2 辺が d の直角三角形を削る
    let removed = (0.5 + 2.0) * 10.0;
    assert!(
        (before - after - removed).abs() < 0.5,
        "{before} - {after} は {removed} のはず"
    );
}

#[test]
fn the_old_single_set_reads_as_one_row_and_is_rewritten_for_the_panel() {
    let p = reference_part().plate.0;
    let old = serde_json::json!({"target": 4, "faces": [format!("f{p}:end")], "radius": "2 mm"});
    let f: Fillet = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(f.rows.len(), 1);
    assert_eq!(f.rows[0].radius, "2 mm");
    // 項目の無い前の文書は、面のまわりの円の辺を入れない(形を変えない)
    assert!(f.rows[0].straight_only);
    assert_eq!(
        f.rows[0].edges,
        vec![kmb_core::Ref::Face {
            feature: FeatureId(4),
            face: format!("f{p}:end")
        }]
    );
    // 画面は前の形を今の形にしてから欄に出す。直した形も同じ機能として読める
    let reg = super::tests::registry();
    let d = reg.descriptor(FILLET).unwrap();
    let new = d.upgraded(&old);
    assert_eq!(new["rows"][0]["radius"], "2 mm", "{new}");
    assert_eq!(new["rows"][0]["straight_only"], true, "{new}");
    let again: Fillet = serde_json::from_value(new.clone()).unwrap();
    assert_eq!(again, f);
    assert_eq!(d.upgraded(&new), new, "今の形はそのまま");
    // 面取りの前の形(edges と distance)も同じ
    let c: Chamfer = serde_json::from_value(serde_json::json!({
        "target": 4, "edges": [["a", "b"]], "distance": "1 mm", "corner": "patch"
    }))
    .unwrap();
    assert_eq!(c.rows[0].distance, "1 mm");
    assert_eq!(c.corner, Corner::Patch);
    assert!(
        matches!(&c.rows[0].edges[0], kmb_core::Ref::Edge { faces, .. } if faces == &["a".to_string(), "b".to_string()])
    );
}
