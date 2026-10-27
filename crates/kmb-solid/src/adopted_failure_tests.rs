//! 押し出し・回転・結合が失敗するか、黙って違う形を出す所(採用分の点検で「直す」と判定した物)。
//! 勾配の押し出しで細い所が閉じる切り取り・中空の球の回転・離れた道具の結合。

use super::*;
use crate::tests::{TestKernel, assert_ok, close, final_mass, rect_sketch, registry};
use kmb_core::{Document, FeatureStatus};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;
use std::f64::consts::PI;

/// ZX 平面(スケッチの x が世界の z、y が世界の x)に、中心を原点にした半径 `outer` の半円から
/// 半径 `inner` の半円を抜いた輪郭(`inner` が 0 なら半円)。直径はスケッチの X 軸に載る。
fn half_ring(b: &mut DocBuilder, outer: f64, inner: f64) -> kmb_core::FeatureId {
    let mut s = SketchBuilder::new(SketchPlane::Zx);
    let c = s.fixed_point(0.0, 0.0);
    let (p0, p1) = (s.fixed_point(outer, 0.0), s.fixed_point(-outer, 0.0));
    s.arc(c, p0, p1);
    if inner > 0.0 {
        let (q0, q1) = (s.fixed_point(inner, 0.0), s.fixed_point(-inner, 0.0));
        s.arc(c, q0, q1);
        s.line(p1, q1);
        s.line(q0, p0);
    } else {
        s.line(p1, p0);
    }
    b.feature(kmb_sketch::SKETCH, "半円", s.build())
}

fn sphere_from(outer: f64, inner: f64) -> Document {
    let mut b = DocBuilder::new();
    let s = half_ring(&mut b, outer, inner);
    b.feature(
        REVOLVE,
        "球",
        Revolve {
            profile: Some(s),
            regions: Vec::new(),
            axis: Axis::SketchX,
            angle: "360 deg".into(),
            operation: Operation::New,
            target: None,
            direction: Direction::Normal,
            angle2: None,
            extent: RevolveExtent::Angle,
            to_face: None,
            ..Default::default()
        },
    );
    b.finish("球")
}

/// 10-25 k8: 半円を直径の軸で回すと球、半円の輪(内と外の半円を直径の 2 本の線でつないだ物)を
/// 回すと中空の球(殻)になる。殻は空洞のある 1 つのボディで、体積は外の球から内の球を引いた分。
#[test]
fn revolving_a_half_ring_about_its_diameter_makes_a_hollow_sphere() {
    let k = TestKernel::new();
    let ball = |r: f64| 4.0 / 3.0 * PI * r.powi(3);
    let m = final_mass(&mut sphere_from(10.0, 0.0), &k);
    assert!(
        close(m.volume, ball(10.0), 2e-3),
        "球 {} vs {}",
        m.volume,
        ball(10.0)
    );
    let mut doc = sphere_from(10.0, 6.0);
    let m = final_mass(&mut doc, &k);
    let want = ball(10.0) - ball(6.0);
    assert!(close(m.volume, want, 2e-3), "殻 {} vs {want}", m.volume);
    assert!(
        close(m.max[0], 10.0, 1e-3) && close(m.min[2], -10.0, 1e-3),
        "{m:?}"
    );
}

/// 板 40×30×10 の上の面に、幅 `w`・長さ 20 の溝の輪郭を描き、勾配 `deg` で下へ 10 切り取る。
fn drafted_slot(w: f64, deg: f64) -> (Document, kmb_core::FeatureId) {
    let mut b = DocBuilder::new();
    let ps = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 40.0, 30.0);
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(ps),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let o = s.fixed_point(0.0, 0.0);
    let (x, y) = (10.0, 15.0 - w / 2.0);
    let c = s.point_from(o, &format!("{x} mm"), &format!("{y} mm"), [x, y]);
    s.rectangle(c, "20 mm", &format!("{w} mm"), [x, y, 20.0, w]);
    let sk = b.feature(kmb_sketch::SKETCH, "溝", s.build());
    let cut = b.feature(
        EXTRUDE,
        "勾配の溝",
        Extrude {
            profile: Some(sk),
            distance: "10 mm".into(),
            direction: Direction::Reverse,
            operation: Operation::Cut,
            target: Some(plate),
            taper: Some(format!("{deg} deg")),
            ..Default::default()
        },
    );
    (b.finish("勾配の溝"), cut)
}

/// 10-25 k1: 勾配を付けた切り取りで、溝の両側の壁が深さの途中で出会う(幅 3、勾配 15° なら
/// 深さ 5.6 で閉じる)。閉じた先は削らず、V の溝(断面は底辺 3・高さ 1.5/tan15° の三角)が残る。
/// 閉じる先まで傾いた壁を延ばして裏返った道具を作らない。
#[test]
fn a_drafted_cut_whose_walls_meet_stops_where_they_meet() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck はテーパーの押し出しを作れない");
    }
    let (mut doc, cut) = drafted_slot(3.0, 15.0);
    let report = doc.recompute(&registry(), &*k);
    let st = report.feature(cut).unwrap();
    assert!(matches!(st, FeatureStatus::Ok { .. }), "{st:?}");
    let removed = 40.0 * 30.0 * 10.0 - final_mass(&mut doc, &k).volume;
    let t = 15f64.to_radians().tan();
    let depth = 1.5 / t;
    // 長さの向きにも同じ勾配で縮むので、くさびの両端は斜めに切れる(すい体の和)
    let want = {
        // 深さ z での断面は (20 − 2 z t) × (3 − 2 z t)。z は 0〜depth
        let a = |z: f64| (20.0 - 2.0 * z * t) * (3.0 - 2.0 * z * t);
        depth / 6.0 * (a(0.0) + 4.0 * a(depth / 2.0) + a(depth))
    };
    assert!(close(removed, want, 2e-3), "{removed} vs {want}");
}

/// 10-25 k7: 結合の道具が対象のボディから離れていると、1 つのボディの中に離れた 2 つの塊ができる。
/// 黙って作らず、離れていることを注意で知らせる(形は作る。後で動かして付けることもあるので)。
/// 面で接して 1 つにつながる結合では知らせない。
#[test]
fn joining_a_tool_that_does_not_touch_the_body_warns() {
    let k = TestKernel::new();
    // 左の箱 10×10×10 に、x から幅 10・高さ h の箱を結合して、離れた塊の注意が出たか
    let warned = |x: f64, h: f64| {
        let mut b = DocBuilder::new();
        let ps = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 10.0, 10.0);
        let a = b.feature(
            EXTRUDE,
            "左",
            Extrude {
                profile: Some(ps),
                distance: "10 mm".into(),
                ..Default::default()
            },
        );
        let s = rect_sketch(&mut b, SketchPlane::Xy, x, 0.0, 10.0, 10.0);
        let join = b.feature(
            EXTRUDE,
            "右",
            Extrude {
                profile: Some(s),
                distance: format!("{h} mm"),
                operation: Operation::Join,
                target: Some(a),
                ..Default::default()
            },
        );
        let mut doc = b.finish("結合");
        let report = doc.recompute(&registry(), &*k);
        assert_ok(&report);
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(join) else {
            panic!("{:?}", report.feature(join));
        };
        kmb_core::warnings_of(outputs)
            .iter()
            .any(|w| w.contains("離れた塊"))
    };
    assert!(warned(20.0, 10.0), "離れた結合");
    if k.is_truck() {
        kmb_contract::skip!("truck は同じ平面で接する形のブーリアンに失敗する(下の面がそろう結合)");
    }
    assert!(!warned(5.0, 12.0), "重なる結合");
    assert!(!warned(10.0, 10.0), "面で接する結合");
}

/// 10-25 k1: 輪郭の一部(幅 2 の細い腕)だけが深さの途中で閉じる勾配の切り取りは、SMLib が閉じて
/// いない道具を作る。黙って何も削らずに通さず、理由を出して止める(直す前は削った体積 0 で通っていた)。
/// 腕の所で閉じた先を削らずに本体を底まで削る形は、壁の出会う所を求めて道具を組む作りが要る(残り)。
#[test]
fn a_drafted_cut_whose_narrow_arm_closes_stops_with_a_reason() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck はテーパーの押し出しを作れない");
    }
    let mut b = DocBuilder::new();
    let ps = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 40.0, 30.0);
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(ps),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    // 本体 20×10(x 10〜30、y 5〜15)の上の辺の真ん中から、幅 2・長さ 8 の腕が出た T の形
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let pts = [
        (10.0, 5.0),
        (30.0, 5.0),
        (30.0, 15.0),
        (21.0, 15.0),
        (21.0, 23.0),
        (19.0, 23.0),
        (19.0, 15.0),
        (10.0, 15.0),
    ]
    .map(|(x, y)| s.fixed_point(x, y));
    for i in 0..pts.len() {
        s.line(pts[i], pts[(i + 1) % pts.len()]);
    }
    let sk = b.feature(kmb_sketch::SKETCH, "T", s.build());
    let cut = b.feature(
        EXTRUDE,
        "勾配の T",
        Extrude {
            profile: Some(sk),
            distance: "8 mm".into(),
            direction: Direction::Reverse,
            operation: Operation::Cut,
            target: Some(plate),
            taper: Some("15 deg".into()),
            ..Default::default()
        },
    );
    let mut doc = b.finish("勾配の T");
    let report = doc.recompute(&registry(), &*k);
    let st = report.feature(cut).unwrap();
    assert!(
        matches!(st, FeatureStatus::Error { message } if message.contains("細い所の壁が途中で出会う")),
        "{st:?}"
    );
}
