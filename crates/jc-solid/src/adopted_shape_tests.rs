//! 他のソフトで起きた形の不具合が、JC では起きないことを確かめる(採用分の点検で「試験だけ」と
//! 判定した物)。抜き勾配の押し出し・面で止めた切り取り・同じ高さの和・傾いた壁の穴・写しの効く
//! ボディ・穴に差したピンの面取り・長穴の縁の面取り。

use super::*;
use crate::tests::{EdgeBench, TestKernel, assert_ok, close, final_mass, rect_sketch, registry};
use jc_core::{Document, FaceInfo};
use jc_samples::sample::{DocBuilder, SketchBuilder};
use jc_sketch::SketchPlane;
use std::f64::consts::PI;

/// 見えているボディの体積(小さい順)。
fn volumes(doc: &mut Document, k: &TestKernel) -> Vec<f64> {
    let reg = registry();
    let report = doc.recompute(&reg, &**k);
    assert_ok(&report);
    let mut v: Vec<f64> = visible_bodies(doc.data(), &reg, &report)
        .into_iter()
        .map(|(_, h)| k.mass(h).unwrap().volume)
        .collect();
    v.sort_by(f64::total_cmp);
    v
}

/// 見えているボディ 1 つの面。
fn faces(doc: &mut Document, k: &TestKernel) -> Vec<FaceInfo> {
    let reg = registry();
    let report = doc.recompute(&reg, &**k);
    assert_ok(&report);
    let bodies = visible_bodies(doc.data(), &reg, &report);
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    k.faces(bodies[0].1).unwrap()
}

/// 外向きが `nz`(+1 か −1)の水平な面の面積の和。
fn flat_area(faces: &[FaceInfo], nz: f64) -> f64 {
    faces
        .iter()
        .filter(|f| f.planar.is_some_and(|p| (p.normal[2] - nz).abs() < 1e-6))
        .map(|f| f.area)
        .sum()
}

/// 高さ `h`、勾配 `deg` で先へ狭まる長方形 `w`×`d` のすい台の体積。
fn frustum(w: f64, d: f64, h: f64, deg: f64) -> f64 {
    let s = h * deg.to_radians().tan();
    let a = |s: f64| (w - 2.0 * s) * (d - 2.0 * s);
    h / 6.0 * (a(0.0) + 4.0 * a(s / 2.0) + a(s))
}

/// 長方形 `[x, y, w, h]` を高さ `t` に押し出した新しいボディ。
fn block(b: &mut DocBuilder, name: &str, [x, y, w, h]: [f64; 4], t: f64) -> jc_core::FeatureId {
    let s = rect_sketch(b, SketchPlane::Xy, x, y, w, h);
    b.feature(
        EXTRUDE,
        name,
        Extrude {
            profile: Some(s),
            distance: format!("{t} mm"),
            ..Default::default()
        },
    )
}

/// 10-20 f2: 角が円弧の輪郭(角を半径 4 で丸めた 30×20)に勾配 5° を付けると、直線も円弧も同じだけ
/// 内へ傾く。断面積は縮む量の 2 次式なので、体積はシンプソンの式で積んだ値とちょうど合い、
/// 上の面は縮んだ輪郭の面積になる。
#[test]
fn a_draft_on_a_rounded_outline_leans_the_arcs_like_the_lines() {
    let k = TestKernel::new();
    if k.is_truck() {
        jc_contract::skip!("truck はテーパーの押し出しを作れない");
    }
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    s.rounded_rectangle(0.0, 0.0, 30.0, 20.0, 4.0);
    let sk = b.feature(jc_sketch::SKETCH, "輪郭", s.build());
    b.feature(
        EXTRUDE,
        "勾配",
        Extrude {
            profile: Some(sk),
            distance: "10 mm".into(),
            taper: Some("5 deg".into()),
            ..Default::default()
        },
    );
    let mut doc = b.finish("勾配");
    let m = final_mass(&mut doc, &k);
    let s = 10.0 * 5f64.to_radians().tan();
    let area = |s: f64| (30.0 - 2.0 * s) * (20.0 - 2.0 * s) - (4.0 - PI) * (4.0 - s).powi(2);
    let want = 10.0 / 6.0 * (area(0.0) + 4.0 * area(s / 2.0) + area(s));
    assert!(close(m.volume, want, 1e-3), "{} vs {want}", m.volume);
    let top = flat_area(&faces(&mut doc, &k), 1.0);
    assert!(close(top, area(s), 1e-3), "上の面 {top} vs {}", area(s));
}

/// 10-20 f3: 勾配と開始のずらしは一緒に使える。輪郭の平面から 5 mm 浮かせて始め、そこから
/// 10 mm の間に勾配 10° で狭まる。
#[test]
fn a_draft_and_a_start_offset_work_together() {
    let k = TestKernel::new();
    if k.is_truck() {
        jc_contract::skip!("truck はテーパーの押し出しを作れない");
    }
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 10.0, 10.0);
    b.feature(
        EXTRUDE,
        "浮いたすい台",
        Extrude {
            profile: Some(s),
            distance: "10 mm".into(),
            start: Start::Offset,
            start_offset: Some("5 mm".into()),
            taper: Some("10 deg".into()),
            ..Default::default()
        },
    );
    let m = final_mass(&mut b.finish("ずらしと勾配"), &k);
    let want = frustum(10.0, 10.0, 10.0, 10.0);
    assert!(close(m.volume, want, 1e-3), "{} vs {want}", m.volume);
    assert!(
        close(m.min[2], 5.0, 1e-6) && close(m.max[2], 15.0, 1e-6),
        "{m:?}"
    );
}

/// 10-20 f24: 板の上の面に描いた輪郭を、勾配を付けて下の面まで(対象まで)切り取ると、止めた面に
/// 薄い皮が残らず抜ける。上と下の面はどちらも穴の分だけ減り、削った体積はすい台。
/// 勾配の付いた押し出しは面にそろう端を延ばさない(jc-cad の `Recipe::Extrude`)ので確かめる。
#[test]
fn a_drafted_cut_to_a_face_leaves_no_skin_on_that_face() {
    let k = TestKernel::new();
    if k.is_truck() {
        jc_contract::skip!("truck はテーパーの押し出しを作れない");
    }
    let mut b = DocBuilder::new();
    let plate = block(&mut b, "板", [0.0, 0.0, 30.0, 20.0], 10.0);
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let o = s.fixed_point(0.0, 0.0);
    let c = s.point_from(o, "10 mm", "5 mm", [10.0, 5.0]);
    s.rectangle(c, "10 mm", "10 mm", [10.0, 5.0, 10.0, 10.0]);
    let sk = b.feature(jc_sketch::SKETCH, "穴の輪郭", s.build());
    b.feature(
        EXTRUDE,
        "勾配の切り取り",
        Extrude {
            profile: Some(sk),
            distance: "1 mm".into(),
            direction: Direction::Reverse,
            extent: Extent::ToFace,
            to_face: Some(ToFace {
                feature: plate,
                face: format!("f{}:start", plate.0),
                vertex: None,
            }),
            operation: Operation::Cut,
            target: Some(plate),
            taper: Some("5 deg".into()),
            ..Default::default()
        },
    );
    let mut doc = b.finish("面で止めた切り取り");
    let removed = 30.0 * 20.0 * 10.0 - final_mass(&mut doc, &k).volume;
    let want = frustum(10.0, 10.0, 10.0, 5.0);
    assert!(close(removed, want, 1e-3), "{removed} vs {want}");
    let f = faces(&mut doc, &k);
    let bottom = 10.0 - 2.0 * 10.0 * 5f64.to_radians().tan();
    let (up, down) = (flat_area(&f, 1.0), flat_area(&f, -1.0));
    assert!(close(up, 600.0 - 100.0, 1e-6), "上の面 {up}");
    assert!(close(down, 600.0 - bottom * bottom, 1e-6), "下の面 {down}");
}

/// 10-20 f25: 同じ高さで半分重なる箱を結合しても、余計な薄い板は残らない。体積は和の 30×20×10、
/// 高さは 10 のまま、細い面も無い。
#[test]
fn joining_a_box_of_the_same_height_leaves_no_extra_plate() {
    let k = TestKernel::new();
    if k.is_truck() {
        jc_contract::skip!("truck は同じ平面で接する形のブーリアンに失敗する(同じ高さの箱の上下の面)");
    }
    let mut b = DocBuilder::new();
    let a = block(&mut b, "左", [0.0, 0.0, 20.0, 20.0], 10.0);
    let s = rect_sketch(&mut b, SketchPlane::Xy, 10.0, 0.0, 20.0, 20.0);
    b.feature(
        EXTRUDE,
        "右",
        Extrude {
            profile: Some(s),
            distance: "10 mm".into(),
            operation: Operation::Join,
            target: Some(a),
            ..Default::default()
        },
    );
    let mut doc = b.finish("同じ高さ");
    let m = final_mass(&mut doc, &k);
    assert!(close(m.volume, 30.0 * 20.0 * 10.0, 1e-6), "{}", m.volume);
    assert!(close(m.max[2], 10.0, 1e-9), "{m:?}");
    let f = faces(&mut doc, &k);
    let thin: Vec<_> = f.iter().filter(|f| f.area < 0.1).collect();
    assert!(thin.is_empty(), "細い面: {thin:?}");
    assert!(close(flat_area(&f, 1.0), 600.0, 1e-6), "{f:?}");
}

/// 10-20 f21: 45° に傾いた壁(水平の厚さ 10)を縦に貫く穴は、途中で止まらず壁の向こうへ抜ける。
/// 縦の線が壁の中を通る長さはどこでも 10 なので、削る体積は π r² × 10。
#[test]
fn a_through_hole_passes_a_slanted_wall() {
    let k = TestKernel::new();
    if k.is_truck() {
        jc_contract::skip!("truck のブーリアンは斜めの面と円筒の交わりを粗く近づけ、削る体積が 0.2% ずれる");
    }
    let mut b = DocBuilder::new();
    // ZX 平面(スケッチの X は世界の z、Y は世界の x)の平行四辺形を y の向きに 20 押し出す
    let mut s = SketchBuilder::new(SketchPlane::Zx);
    let ids: Vec<_> = [[-20.0, 0.0], [-20.0, 10.0], [0.0, 30.0], [0.0, 20.0]]
        .iter()
        .map(|p| s.fixed_point(p[0], p[1]))
        .collect();
    for i in 0..4 {
        s.line(ids[i], ids[(i + 1) % 4]);
    }
    let sk = b.feature(jc_sketch::SKETCH, "傾いた壁", s.build());
    let wall = b.feature(
        EXTRUDE,
        "壁",
        Extrude {
            profile: Some(sk),
            distance: "20 mm".into(),
            ..Default::default()
        },
    );
    // 壁の上(z = 0 の XY 平面)の点から下へ貫通
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let p = s.fixed_point(15.0, 10.0);
    let hs = b.feature(jc_sketch::SKETCH, "穴の位置", s.build());
    b.feature(
        HOLE,
        "穴",
        Hole {
            points: vec![jc_core::PointId(p.0)],
            diameter: "5 mm".into(),
            ..Hole::new(wall, hs)
        },
    );
    let mut doc = b.finish("傾いた壁の穴");
    let m = final_mass(&mut doc, &k);
    assert!(close(m.min[1], 0.0, 1e-6) && close(m.max[1], 20.0, 1e-6), "{m:?}");
    let removed = 10.0 * 20.0 * 20.0 - m.volume;
    let want = PI * 2.5 * 2.5 * 10.0;
    assert!(close(removed, want, 1e-3), "{removed} vs {want}");
}

/// 10-20 f22: 穴を並べた写しは、穴を開けたボディだけを削る。2 つ目の写しは隣の別のボディの上に
/// 来るが、隣のボディは削らない(元の板には穴が 1 つだけ)。
#[test]
fn pattern_copies_cut_only_the_body_of_their_feature() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let plate = block(&mut b, "板", [0.0, 0.0, 30.0, 20.0], 10.0);
    block(&mut b, "隣の板", [40.0, 0.0, 30.0, 20.0], 10.0);
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let o = s.fixed_point(0.0, 0.0);
    let p = s.point_from(o, "15 mm", "10 mm", [15.0, 10.0]);
    let hs = b.feature(jc_sketch::SKETCH, "穴の位置", s.build());
    let hole = b.feature(
        HOLE,
        "穴",
        Hole {
            points: vec![jc_core::PointId(p.0)],
            diameter: "5 mm".into(),
            ..Hole::new(plate, hs)
        },
    );
    b.feature(
        LINEAR_PATTERN,
        "穴の並び",
        LinearPattern {
            feature: vec![hole],
            direction: [1.0, 0.0, 0.0],
            spacing: "40 mm".into(),
            count: "2".into(),
            ..Default::default()
        },
    );
    let v = volumes(&mut b.finish("写しと隣の板"), &k);
    let one = 30.0 * 20.0 * 10.0;
    let want = one - PI * 2.5 * 2.5 * 10.0;
    assert_eq!(v.len(), 2, "{v:?}");
    assert!(close(v[0], want, 1e-3), "穴の板 {} vs {want}", v[0]);
    assert!(close(v[1], one, 1e-9), "隣の板 {}", v[1]);
}

/// 10-23 k4: 穴(径 10.4)に差したピン(径 10、上の面は板と同じ高さ)の上の縁を 1 mm 面取りしても、
/// 削れるのはピンだけで、すき間 0.2 mm の先の穴の壁は削れない。
#[test]
fn chamfering_a_pin_in_a_hole_leaves_the_hole_wall() {
    let k = TestKernel::new();
    if k.is_truck() {
        jc_contract::skip!("truck は円の辺を返さない");
    }
    let mut b = DocBuilder::new();
    let plate = block(&mut b, "板", [0.0, 0.0, 30.0, 30.0], 10.0);
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let o = s.fixed_point(0.0, 0.0);
    let p = s.point_from(o, "15 mm", "15 mm", [15.0, 15.0]);
    let hs = b.feature(jc_sketch::SKETCH, "穴の位置", s.build());
    b.feature(
        HOLE,
        "穴",
        Hole {
            points: vec![jc_core::PointId(p.0)],
            diameter: "10.4 mm".into(),
            ..Hole::new(plate, hs)
        },
    );
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    let c = s.point_from(o, "15 mm", "15 mm", [15.0, 15.0]);
    s.circle(c, "10 mm", 5.0);
    let ps = b.feature(jc_sketch::SKETCH, "ピンの輪郭", s.build());
    let pin = block_from(&mut b, ps);
    let sel = EdgeSelection {
        edges: vec![],
        faces: vec![format!("f{}:end", pin.0)],
        arcs: true,
    };
    let mut doc = b.finish("ピンの面取り");
    let v0 = volumes(&mut doc, &k);
    let data = serde_json::to_value(Chamfer::one(pin, sel, "1 mm", false, Default::default()));
    let id = doc.alloc_feature_id();
    doc.apply(jc_core::Transaction::new(
        "面取り",
        vec![jc_core::Op::InsertFeature {
            index: None,
            entry: jc_core::FeatureEntry {
                id,
                type_id: CHAMFER.into(),
                name: "面取り".into(),
                data: data.unwrap(),
                suppressed: false,
            },
        }],
    ))
    .unwrap();
    let v1 = volumes(&mut doc, &k);
    // 小さい方がピン、大きい方が板
    assert!(close(v1[1], v0[1], 1e-9), "板 {} → {}", v0[1], v1[1]);
    let want = 0.5 * 2.0 * PI * (5.0 - 1.0 / 3.0);
    assert!(close(v0[0] - v1[0], want, 1e-2), "ピン {} vs {want}", v0[0] - v1[0]);
}

fn block_from(b: &mut DocBuilder, profile: jc_core::FeatureId) -> jc_core::FeatureId {
    b.feature(
        EXTRUDE,
        "ピン",
        Extrude {
            profile: Some(profile),
            distance: "10 mm".into(),
            ..Default::default()
        },
    )
}

/// 10-20 f10: 長穴(長さ 30・幅 10、両端は半円)のポケットの上の縁を 1 mm 面取りすると、直線と
/// 半円の継ぎ目で崩れず一周する。削る体積は、直線は断面 × 長さ、半円は断面 × 重心の回る長さ
/// (へこんだ縁なので重心は半径の外側 d/3)。角を丸めた長方形のポケットは edge_tests にある。
#[test]
fn a_chamfer_runs_round_the_rim_of_a_slot_pocket() {
    let k = TestKernel::new();
    if k.is_truck() {
        jc_contract::skip!("truck は円の辺を返さない");
    }
    let mut b = DocBuilder::new();
    let plate = block(&mut b, "板", [0.0, 0.0, 60.0, 40.0], 10.0);
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let [c1, c2] = [[20.0, 20.0], [40.0, 20.0]].map(|p| s.fixed_point(p[0], p[1]));
    let [a, b1, c, d] =
        [[20.0, 15.0], [40.0, 15.0], [40.0, 25.0], [20.0, 25.0]].map(|p| s.fixed_point(p[0], p[1]));
    let bottom = s.line(a, b1);
    s.arc(c2, b1, c);
    s.line(c, d);
    s.arc(c1, d, a);
    let sk = b.feature(jc_sketch::SKETCH, "長穴", s.build());
    let pocket = b.feature(
        EXTRUDE,
        "ポケット",
        Extrude {
            profile: Some(sk),
            distance: "4 mm".into(),
            direction: Direction::Reverse,
            operation: Operation::Cut,
            target: Some(plate),
            ..Default::default()
        },
    );
    let mut bench = EdgeBench::new(b.finish("長穴のポケット"), pocket);
    let after = bench.after(
        CHAMFER,
        serde_json::json!({
            "rows": [{
                "edges": {"kind": "edge", "feature": pocket.0, "faces": [
                    format!("f{}:end", plate.0),
                    format!("f{}:side:e{}", pocket.0, bottom.0),
                ]},
                "distance": "1 mm",
            }],
            "chain": true,
            "method": "blend",
        }),
    );
    let area = 0.5;
    let want = area * 2.0 * 20.0 + area * 2.0 * PI * (5.0 + 1.0 / 3.0);
    let removed = bench.before - after;
    assert!((removed - want).abs() < 0.01, "{removed} vs {want}");
}

/// 10-24 k12: 壊れたファイルや AI の打ち込みで桁違いの長さが入っても、計算は止まらず、その機能
/// (とそれを使う機能)だけが理由つきの誤りになる。ほかのボディは計算される。
#[test]
fn a_huge_length_stops_only_its_own_feature() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let ok = block(&mut b, "箱", [0.0, 0.0, 10.0, 10.0], 5.0);
    let s = rect_sketch(&mut b, SketchPlane::Xy, 20.0, 0.0, 10.0, 10.0);
    let tall = b.feature(
        EXTRUDE,
        "高すぎる箱",
        Extrude {
            profile: Some(s),
            distance: "1e300 mm".into(),
            ..Default::default()
        },
    );
    // スケッチの寸法が桁違い(1e12 mm)
    let mut sk = SketchBuilder::new(SketchPlane::Xy);
    let o = sk.fixed_point(40.0, 0.0);
    sk.rectangle(o, "1e12 mm", "10 mm", [40.0, 0.0, 50.0, 10.0]);
    let wide = b.feature(jc_sketch::SKETCH, "広すぎるスケッチ", sk.build());
    let mut doc = b.finish("桁違いの値");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    let message = |id| match report.feature(id) {
        Some(jc_core::FeatureStatus::Error { message }) => message.clone(),
        other => panic!("{id} は誤りになる: {other:?}"),
    };
    assert!(matches!(
        report.feature(ok),
        Some(jc_core::FeatureStatus::Ok { .. })
    ));
    let m = message(tall);
    assert!(m.contains("大きすぎます(1000000 mm まで)"), "{m}");
    let m = message(wide);
    assert!(m.contains("大きすぎます"), "{m}");
    assert_eq!(visible_bodies(doc.data(), &reg, &report).len(), 1);
}
