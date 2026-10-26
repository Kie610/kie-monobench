//! 押し出しの薄い押し出し・始まりのずらし・テーパー、回転の向きと「対象まで」、穴の「対象まで」
//! を確かめる。

use super::*;
use crate::tests::{TestKernel, assert_ok, close, final_mass, rect_sketch, registry};
use kmb_core::FeatureStatus;
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;

/// 原点から始まる折れ線(開いた線)だけのスケッチ。
fn polyline_sketch(b: &mut DocBuilder, pts: &[[f64; 2]]) -> kmb_core::FeatureId {
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let ids: Vec<_> = pts.iter().map(|p| s.fixed_point(p[0], p[1])).collect();
    for w in ids.windows(2) {
        s.line(w[0], w[1]);
    }
    b.feature(kmb_sketch::SKETCH, "線", s.build())
}

fn extrude(profile: kmb_core::FeatureId, distance: &str) -> Extrude {
    Extrude {
        profile: Some(profile),
        distance: distance.into(),
        ..Default::default()
    }
}

#[test]
fn a_thin_extrude_turns_an_open_line_into_a_wall() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let s = polyline_sketch(&mut b, &[[0.0, 0.0], [20.0, 0.0]]);
    b.feature(
        EXTRUDE,
        "壁",
        Extrude {
            thin: Thin::Center,
            thickness: Some("2 mm".into()),
            ..extrude(s, "10 mm")
        },
    );
    let mut doc = b.finish("薄い押し出し");
    let m = final_mass(&mut doc, &k);
    assert!(close(m.volume, 20.0 * 2.0 * 10.0, 1e-6), "{}", m.volume);
    assert!(close(m.min[1], -1.0, 1e-6) && close(m.max[1], 1.0, 1e-6));

    // 内側(たどる向きの左、+y)に付けた L 字の壁は 1 つのボディになる
    let mut b = DocBuilder::new();
    let s = polyline_sketch(&mut b, &[[0.0, 0.0], [20.0, 0.0], [20.0, 10.0]]);
    b.feature(
        EXTRUDE,
        "L 字の壁",
        Extrude {
            thin: Thin::Inside,
            ..extrude(s, "5 mm")
        },
    );
    let mut doc = b.finish("L 字");
    let m = final_mass(&mut doc, &k);
    // 既定の厚さ 2 mm。左は +y と −x の側なので、外形は x 0〜20、y 0〜10
    let v = (20.0 * 2.0 + 8.0 * 2.0) * 5.0;
    assert!(close(m.volume, v, 1e-6), "{} vs {v}", m.volume);
    assert!(
        close(m.min[0], 0.0, 1e-6) && close(m.max[0], 20.0, 1e-6),
        "{m:?}"
    );
    assert!(
        close(m.min[1], 0.0, 1e-6) && close(m.max[1], 10.0, 1e-6),
        "{m:?}"
    );
}

#[test]
fn a_thin_extrude_of_a_closed_profile_makes_a_frame() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 30.0, 20.0);
    b.feature(
        EXTRUDE,
        "枠",
        Extrude {
            thin: Thin::Inside,
            thickness: Some("2 mm".into()),
            ..extrude(s, "5 mm")
        },
    );
    let mut doc = b.finish("枠");
    let m = final_mass(&mut doc, &k);
    let v = (30.0 * 20.0 - 26.0 * 16.0) * 5.0;
    assert!(close(m.volume, v, 1e-6), "{} vs {v}", m.volume);
    assert!(close(m.max[0], 30.0, 1e-6) && close(m.min[0], 0.0, 1e-6));
}

#[test]
fn the_start_offset_moves_the_extrude_away_from_the_sketch() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 10.0, 10.0);
    b.feature(
        EXTRUDE,
        "浮いた柱",
        Extrude {
            start: Start::Offset,
            start_offset: Some("5 mm".into()),
            ..extrude(s, "10 mm")
        },
    );
    let mut doc = b.finish("開始のオフセット");
    let m = final_mass(&mut doc, &k);
    assert!(close(m.volume, 1000.0, 1e-6));
    assert!(
        close(m.min[2], 5.0, 1e-6) && close(m.max[2], 15.0, 1e-6),
        "{m:?}"
    );
}

#[test]
fn a_tapered_extrude_narrows_toward_its_end() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 10.0, 10.0);
    let id = b.feature(
        EXTRUDE,
        "すい台",
        Extrude {
            taper: Some("10 deg".into()),
            ..extrude(s, "10 mm")
        },
    );
    let mut doc = b.finish("テーパー");
    if k.is_truck() {
        // truck のカーネルにはテーパーの口が無い。理由つきで止まる
        let report = doc.recompute(&registry(), &*k);
        let st = report.feature(id).unwrap();
        assert!(
            matches!(st, FeatureStatus::Error { message } if message.contains("テーパー")),
            "{st:?}"
        );
        kmb_contract::skip!("truck はテーパーの押し出しを作れない(理由つきで断るのは確かめた)");
    }
    let m = final_mass(&mut doc, &k);
    let top = 10.0 - 2.0 * 10.0 * 10f64.to_radians().tan();
    let v = 10.0 / 3.0 * (100.0 + top * top + 10.0 * top);
    assert!(close(m.volume, v, 1e-3), "{} vs {v}", m.volume);
}

/// ZX 平面の長方形(世界の x 10〜15、z 0〜8)を、世界の z 軸(スケッチの X 軸)まわりに回す。
fn ring(direction: Direction, angle: &str, angle2: Option<&str>) -> Revolve {
    Revolve {
        profile: kmb_core::FeatureId(0),
        regions: Vec::new(),
        axis: Axis::SketchX,
        angle: angle.into(),
        operation: Operation::New,
        target: None,
        direction,
        angle2: angle2.map(String::from),
        extent: RevolveExtent::Angle,
        to_face: None,
    }
}

#[test]
fn a_revolve_turns_both_ways_symmetric_or_by_two_angles() {
    let k = TestKernel::new();
    let sector = |deg: f64| deg / 360.0 * std::f64::consts::PI * (225.0 - 100.0) * 8.0;
    let run = |r: Revolve| {
        let mut b = DocBuilder::new();
        let s = rect_sketch(&mut b, SketchPlane::Zx, 0.0, 10.0, 8.0, 5.0);
        b.feature(REVOLVE, "輪", Revolve { profile: s, ..r });
        let mut doc = b.finish("回転");
        final_mass(&mut doc, &k)
    };
    // 片側 90 度: 輪郭(y = 0)から +y の側だけ
    let m = run(ring(Direction::Normal, "90 deg", None));
    assert!(close(m.volume, sector(90.0), 2e-3), "{}", m.volume);
    assert!(m.min[1] > -1e-3 && m.max[1] > 14.0, "{m:?}");
    // 逆向き: −y の側
    let m = run(ring(Direction::Reverse, "90 deg", None));
    assert!(m.max[1] < 1e-3 && m.min[1] < -14.0, "{m:?}");
    // 対称: 両側へ 45 度ずつ(y の範囲が ±15·sin45)
    let m = run(ring(Direction::Symmetric, "90 deg", None));
    assert!(close(m.volume, sector(90.0), 2e-3), "{}", m.volume);
    let s45 = 15.0 * std::f64::consts::FRAC_1_SQRT_2;
    assert!(
        close(m.max[1], s45, 1e-2) && close(m.min[1], -s45, 1e-2),
        "{m:?}"
    );
    // 両側(別の角度): 表 90、裏 45
    let m = run(ring(Direction::TwoSides, "90 deg", Some("45 deg")));
    assert!(close(m.volume, sector(135.0), 2e-3), "{}", m.volume);
    // 合わせて一周を超えたら一周
    let m = run(ring(Direction::TwoSides, "270 deg", Some("180 deg")));
    assert!(close(m.volume, sector(360.0), 2e-3), "{}", m.volume);
}

#[test]
fn a_revolve_stops_at_a_face_that_holds_its_axis() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!(
            "truck は同じ平面で接する形のブーリアンに失敗する(輪と箱の上下の面が同じ高さ)。\
             通るかは計算の末の桁しだいで、Windows では落ちる"
        );
    }
    let mut b = DocBuilder::new();
    // x −20〜0、y 0〜20、z 0〜8 の箱。x = 0 の面は z 軸を含む
    let bs = rect_sketch(&mut b, SketchPlane::Xy, -20.0, 0.0, 20.0, 20.0);
    let block = b.feature(EXTRUDE, "箱", extrude(bs, "8 mm"));
    let s = rect_sketch(&mut b, SketchPlane::Zx, 0.0, 10.0, 8.0, 5.0);
    let face = {
        let mut d = b.finish("名前を見る");
        let reg = registry();
        let report = d.recompute(&reg, &*k);
        assert_ok(&report);
        let body = visible_bodies(d.data(), &reg, &report)[0].1;
        k.faces(body)
            .unwrap()
            .into_iter()
            .find(|f| {
                f.planar
                    .is_some_and(|p| p.normal[0] > 0.99 && p.point[0].abs() < 1e-6)
            })
            .unwrap()
            .name
    };
    let mut b = DocBuilder::new();
    let bs = rect_sketch(&mut b, SketchPlane::Xy, -20.0, 0.0, 20.0, 20.0);
    let block2 = b.feature(EXTRUDE, "箱", extrude(bs, "8 mm"));
    assert_eq!(block, block2);
    let s2 = rect_sketch(&mut b, SketchPlane::Zx, 0.0, 10.0, 8.0, 5.0);
    assert_eq!(s, s2);
    b.feature(
        REVOLVE,
        "面までの輪",
        Revolve {
            profile: s2,
            operation: Operation::Join,
            target: Some(block2),
            extent: RevolveExtent::ToFace,
            to_face: Some(RevolveStop {
                feature: block2,
                face,
                vertex: None,
            }),
            ..ring(Direction::Normal, "30 deg", None)
        },
    );
    let mut doc = b.finish("対象まで");
    let m = final_mass(&mut doc, &k);
    // +x から +y を回って x = 0 の面(y > 0 の側)まで 90 度
    let quarter = 0.25 * std::f64::consts::PI * (225.0 - 100.0) * 8.0;
    let v = 20.0 * 20.0 * 8.0 + quarter;
    assert!(close(m.volume, v, 2e-3), "{} vs {v}", m.volume);
}

#[test]
fn a_hole_can_stop_at_a_parallel_face() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 30.0, 20.0);
    let plate = b.feature(EXTRUDE, "板", extrude(s, "10 mm"));
    let mut hs = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let o = hs.fixed_point(0.0, 0.0);
    let c = hs.point_from(o, "15 mm", "10 mm", [15.0, 10.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "穴の位置", hs.build());
    let hole = b.feature(
        HOLE,
        "穴",
        Hole {
            points: vec![kmb_core::PointId(c.0)],
            diameter: "6 mm".into(),
            depth: Some("3 mm".into()),
            extent: HoleExtent::ToFace,
            to_face: Some(HoleStop {
                feature: plate,
                face: format!("f{}:start", plate.0),
                vertex: None,
            }),
            ..Hole::new(plate, sk)
        },
    );
    let mut doc = b.finish("穴の対象まで");
    let m = final_mass(&mut doc, &k);
    // 深さの欄(3 mm)は使わず、下の面(z = 0)まで 10 mm
    let v = 30.0 * 20.0 * 10.0 - std::f64::consts::PI * 9.0 * 10.0;
    assert!(close(m.volume, v, 2e-3), "{} vs {v}", m.volume);

    // 穴の面と平行でない面(横の面)は理由つきで止まる
    let reg = registry();
    let mut data = doc.data().feature(hole).unwrap().data.clone();
    let report = doc.recompute(&reg, &*k);
    let side = k
        .faces(visible_bodies(doc.data(), &reg, &report)[0].1)
        .unwrap()
        .into_iter()
        .find(|f| f.planar.is_some_and(|p| p.normal[0] > 0.99))
        .unwrap()
        .name;
    data["to_face"]["face"] = serde_json::json!(side);
    doc.apply(kmb_core::Transaction::new(
        "止める面",
        vec![kmb_core::Op::SetFeatureData { id: hole, data }],
    ))
    .unwrap();
    let report = doc.recompute(&reg, &*k);
    let st = report.feature(hole).unwrap();
    assert!(
        matches!(st, FeatureStatus::Error { message } if message.contains("平行")),
        "{st:?}"
    );
}

/// 板 A(x 0〜30、y 0〜20、z 0〜10)と、下の 4 辺を 4 mm 面取りした箱 B(x 40〜60)。板 A の上面の
/// (15, 2) に径 2 の穴を `stop` まで掘る(`stop` が無ければ穴なし)。B の面取りの面は A の下まで
/// 広げると平面 y + z = 4 になる。
fn plate_and_slanted(stop: Option<HoleStop>) -> (kmb_core::Document, kmb_core::FeatureId) {
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 30.0, 20.0);
    let plate = b.feature(EXTRUDE, "板", extrude(s, "10 mm"));
    let s = rect_sketch(&mut b, SketchPlane::Xy, 40.0, 0.0, 20.0, 20.0);
    let mut e = extrude(s, "10 mm");
    e.operation = Operation::New;
    let block = b.feature(EXTRUDE, "箱", e);
    let ch = b.feature(
        CHAMFER,
        "面取り",
        serde_json::json!({"target": block.0, "kind": "equal", "rows": [{
            "edges": [{"kind": "face", "feature": block.0, "face": format!("f{}:start", block.0)}],
            "distance": "4 mm",
        }]}),
    );
    if let Some(mut t) = stop {
        t.feature = ch;
        let mut hs = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
        let o = hs.fixed_point(0.0, 0.0);
        let c = hs.point_from(o, "15 mm", "2 mm", [15.0, 2.0]);
        let sk = b.feature(kmb_sketch::SKETCH, "穴の位置", hs.build());
        b.feature(
            HOLE,
            "穴",
            Hole {
                points: vec![kmb_core::PointId(c.0)],
                diameter: "2 mm".into(),
                extent: HoleExtent::ToFace,
                to_face: Some(t),
                ..Hole::new(plate, sk)
            },
        );
    }
    (b.finish("斜めの面と頂点まで"), ch)
}

#[test]
fn a_hole_can_stop_at_a_slanted_face_or_a_vertex() {
    if TestKernel::new().is_truck() {
        kmb_contract::skip!(
            "truck の面取りは角の位置が 1〜3 µm ずれ、止める頂点(z = 4)が見つからない"
        );
    }
    // レビュー 2 回目 M5a: 斜めの平らな面なら底が面に沿い、頂点ならその点の高さまで
    let k = TestKernel::new();
    let reg = registry();
    let (mut doc, ch) = plate_and_slanted(None);
    let report = doc.recompute(&reg, &*k);
    assert_ok(&report);
    let body = match report.feature(ch) {
        Some(FeatureStatus::Ok { outputs, .. }) => match outputs.get("body") {
            Some(kmb_core::Value::Shape(h)) => *h,
            o => panic!("{o:?}"),
        },
        o => panic!("{o:?}"),
    };
    let r = std::f64::consts::FRAC_1_SQRT_2;
    let slanted = k
        .faces(body)
        .unwrap()
        .into_iter()
        .find(|f| {
            f.planar.is_some_and(|p| {
                let n = kmb_core::kernel::normalize(p.normal);
                (n[1] + r).abs() < 1e-6 && (n[2] + r).abs() < 1e-6
            })
        })
        .expect("y = 0 の側の面取りの面")
        .name;
    let corner = kmb_core::vertices(&*k, body)
        .into_iter()
        .find(|v| v.at[1].abs() < 1e-6 && (v.at[2] - 4.0).abs() < 1e-6)
        .expect("面取りの上の角(z = 4)")
        .name;
    let plate_volume = 30.0 * 20.0 * 10.0;
    let removed = |stop: HoleStop| {
        let (mut doc, _) = plate_and_slanted(Some(stop));
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
        let (_, a) = visible_bodies(doc.data(), &reg, &report)
            .into_iter()
            .find(|(f, _)| doc.data().feature(*f).is_some_and(|e| e.type_id == HOLE))
            .expect("穴の後の板");
        plate_volume - k.volume(a).unwrap()
    };
    let pi = std::f64::consts::PI;
    // 斜めの面: 穴の底は z = 4 - y(y 1〜3)。深さは平均 8 mm
    let v = removed(HoleStop {
        feature: ch,
        face: slanted,
        vertex: None,
    });
    assert!(close(v, pi * 8.0, 2e-2), "{v} vs {}", pi * 8.0);
    // 頂点(z = 4): 深さ 6 mm のまっすぐな穴
    let v = removed(HoleStop {
        feature: ch,
        face: String::new(),
        vertex: Some(corner),
    });
    assert!(close(v, pi * 6.0, 2e-2), "{v} vs {}", pi * 6.0);
}

#[test]
fn a_revolve_and_an_extrude_can_stop_at_a_vertex() {
    // レビュー 2 回目 M5a: 頂点は、回転なら軸とその点を含む平面、押し出しなら輪郭と平行な平面まで
    let k = TestKernel::new();
    let reg = registry();
    // 小さな箱 x −25〜−20、y 20〜25、z 0〜8。角 (−20, 20, 8) は z 軸から見て 135 度の向き
    let make = |stop: Option<(String, bool)>| {
        let mut b = DocBuilder::new();
        let bs = rect_sketch(&mut b, SketchPlane::Xy, -25.0, 20.0, 5.0, 5.0);
        let block = b.feature(EXTRUDE, "箱", extrude(bs, "8 mm"));
        if let Some((vertex, revolve)) = stop {
            if revolve {
                let s = rect_sketch(&mut b, SketchPlane::Zx, 0.0, 10.0, 8.0, 5.0);
                b.feature(
                    REVOLVE,
                    "頂点までの輪",
                    Revolve {
                        profile: s,
                        operation: Operation::Join,
                        target: Some(block),
                        extent: RevolveExtent::ToFace,
                        to_face: Some(RevolveStop {
                            feature: block,
                            face: String::new(),
                            vertex: Some(vertex),
                        }),
                        ..ring(Direction::Normal, "30 deg", None)
                    },
                );
            } else {
                // 箱の外の z = 0 の 2×2 の正方形を、角の高さ(z = 8)まで
                let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 2.0, 2.0);
                let mut e = extrude(s, "1 mm");
                e.operation = Operation::Join;
                e.target = Some(block);
                e.extent = Extent::ToFace;
                e.to_face = Some(ToFace {
                    feature: block,
                    face: String::new(),
                    vertex: Some(vertex),
                    datum: false,
                });
                b.feature(EXTRUDE, "頂点までの柱", e);
            }
        }
        b.finish("頂点まで")
    };
    let corner = {
        let mut d = make(None);
        let report = d.recompute(&reg, &*k);
        assert_ok(&report);
        let body = visible_bodies(d.data(), &reg, &report)[0].1;
        kmb_core::vertices(&*k, body)
            .into_iter()
            .find(|v| {
                (v.at[0] + 20.0).abs() < 1e-6
                    && (v.at[1] - 20.0).abs() < 1e-6
                    && (v.at[2] - 8.0).abs() < 1e-6
            })
            .expect("角 (−20, 20, 8)")
            .name
    };
    let m = final_mass(&mut make(Some((corner.clone(), true))), &k);
    let sector = 135.0 / 360.0 * std::f64::consts::PI * (225.0 - 100.0) * 8.0;
    let v = 5.0 * 5.0 * 8.0 + sector;
    assert!(close(m.volume, v, 2e-3), "{} vs {v}", m.volume);
    // 押し出し: 柱は z 0 から角の高さ 8 まで(距離の欄の 1 mm は使わない)
    let m = final_mass(&mut make(Some((corner, false))), &k);
    let v = 5.0 * 5.0 * 8.0 + 2.0 * 2.0 * 8.0;
    assert!(close(m.volume, v, 1e-6), "{} vs {v}", m.volume);
}
