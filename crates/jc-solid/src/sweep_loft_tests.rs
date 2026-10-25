//! スイープとロフト(M7)を、文書と実際のカーネルで通す。

use super::tests::{TestKernel, assert_ok, close, final_mass, registry};
use super::*;
use jc_cad_sweep::{LOFT, Loft, MAX_SECTIONS, PathRef, SWEEP, Section, Sweep};
use jc_core::{FeatureEntry, FeatureId, Op, Transaction};
use jc_samples::sample::{DocBuilder, SketchBuilder};
use jc_samples::sweep_loft_sample;
use std::f64::consts::PI;

fn add(
    doc: &mut jc_core::Document,
    type_id: &str,
    name: &str,
    data: serde_json::Value,
) -> FeatureId {
    let id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "足す",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id,
                type_id: type_id.into(),
                name: name.into(),
                data,
                suppressed: false,
            },
        }],
    ))
    .unwrap();
    id
}

/// 直径 6 の円を L の字(上へ 40、横へ 30)に沿って動かすと、体積は 断面 × 中心の線の長さ。
/// 経路の寸法(パラメータ length)を変えると、作り直した形も変わる。ミラーは道具を写して作る。
#[test]
fn a_sweep_follows_its_path_and_its_dimensions() {
    let k = TestKernel::new();
    if k.is_truck() {
        jc_contract::skip!("truck はスイープを作れない");
    }
    let s = sweep_loft_sample();
    let mut doc = s.doc;
    let data = Sweep {
        profile: s.profile,
        regions: vec![],
        path: Some(PathRef {
            sketch: s.path,
            entity: None,
        }),
        operation: Operation::New,
        target: None,
        twist: None,
        scale: None,
    };
    let sw = add(
        &mut doc,
        SWEEP,
        "スイープ1",
        serde_json::to_value(&data).unwrap(),
    );
    let area = PI * 9.0;
    let m = final_mass(&mut doc, &k);
    assert!(close(m.volume, area * 70.0, 1e-4), "{}", m.volume);
    assert!(close(m.max[2], 43.0, 1e-3), "{:?}", m.max);
    // 経路の長さを 60 にすると、上へ 60 まで伸びる
    let id = doc.data().param_by_name("length").unwrap().id;
    doc.apply(Transaction::new(
        "長さ",
        vec![Op::SetParamExpr {
            id,
            expr: "60 mm".into(),
        }],
    ))
    .unwrap();
    let m = final_mass(&mut doc, &k);
    assert!(close(m.volume, area * 90.0, 1e-4), "{}", m.volume);
    // 原点の YZ 平面で写す(x が負の側へ同じ管。上へ伸びる所は元の管と重なるので、足した体積は
    // 2 倍より、重なる縦の管(断面 × 60)とその上の角の分だけ少ない)
    add(
        &mut doc,
        MIRROR,
        "写し",
        serde_json::json!({"feature": sw.0, "plane_ref": {"kind": "origin_yz"}}),
    );
    let m = final_mass(&mut doc, &k);
    assert!(
        m.volume > area * (180.0 - 60.0 - 6.0) && m.volume < area * (180.0 - 60.0),
        "{}",
        m.volume
    );
    assert!(close(m.min[0], -30.0, 1e-3), "{:?}", m.min);
}

/// ねじれと終わりの大きさの欄。経路に中心を置いた円はねじっても体積が変わらず、終わりを半分にすると
/// 細くなる。倍率に長さの単位を書いた時と、閉じた経路は理由つきで止まる。
#[test]
fn a_sweep_twists_and_narrows_by_its_fields() {
    let k = TestKernel::new();
    if k.is_truck() {
        jc_contract::skip!("truck はスイープを作れない");
    }
    let s = sweep_loft_sample();
    let mut doc = s.doc;
    let data = |twist: &str, scale: &str| {
        let opt = |v: &str| (!v.is_empty()).then(|| v.to_string());
        serde_json::to_value(Sweep {
            profile: s.profile,
            regions: vec![],
            path: Some(PathRef {
                sketch: s.path,
                entity: None,
            }),
            operation: Operation::New,
            target: None,
            twist: opt(twist),
            scale: opt(scale),
        })
        .unwrap()
    };
    let sw = add(&mut doc, SWEEP, "スイープ1", data("360", ""));
    let area = PI * 9.0;
    let m = final_mass(&mut doc, &k);
    assert!(close(m.volume, area * 70.0, 1e-3), "{}", m.volume);
    let reg = registry();
    for (twist, scale, want) in [
        ("90 deg", "0.5", None),
        ("", "5 mm", Some("単位の無い倍率")),
        ("", "0", Some("0 より大きく")),
    ] {
        doc.apply(Transaction::new(
            "直す",
            vec![Op::SetFeatureData {
                id: sw,
                data: data(twist, scale),
            }],
        ))
        .unwrap();
        match want {
            None => {
                // 断面は始まりの 1 倍から終わりの 0.5 倍へ細くなる(管の体積の 1/4 より多く 1 倍より少ない)
                let m = final_mass(&mut doc, &k);
                assert!(
                    m.volume < area * 70.0 * 0.6 && m.volume > area * 70.0 / 4.0,
                    "{}",
                    m.volume
                );
            }
            Some(want) => {
                let report = doc.recompute(&reg, &*k);
                let f = report.features.iter().find(|f| f.id == sw).unwrap();
                let text = format!("{:?}", f.status);
                assert!(text.contains(want), "{text}");
            }
        }
    }
}

/// 四角とその上 30 mm の円をつなぐ。同じ平面の断面を続けると、理由つきで止まる。
#[test]
fn a_loft_joins_a_square_to_a_circle_and_refuses_sections_on_one_plane() {
    let k = TestKernel::new();
    if k.is_truck() {
        jc_contract::skip!("truck はロフトを作れない");
    }
    let s = sweep_loft_sample();
    let mut doc = s.doc;
    let section = |p: FeatureId| Section {
        profile: p,
        regions: vec![],
    };
    let data = Loft {
        sections: vec![section(s.square), section(s.circle)],
        rails: vec![],
        operation: Operation::New,
        target: None,
    };
    let lf = add(
        &mut doc,
        LOFT,
        "ロフト1",
        serde_json::to_value(&data).unwrap(),
    );
    let m = final_mass(&mut doc, &k);
    assert!(
        m.volume > PI * 64.0 * 30.0 && m.volume < 400.0 * 30.0,
        "{}",
        m.volume
    );
    assert!(close(m.max[2], 30.0, 1e-3), "{:?}", m.max);
    // 四角と、同じ XY 平面の断面の円(原点の円)
    let bad = Loft {
        sections: vec![section(s.square), section(s.profile)],
        ..data
    };
    doc.apply(Transaction::new(
        "直す",
        vec![Op::SetFeatureData {
            id: lf,
            data: serde_json::to_value(&bad).unwrap(),
        }],
    ))
    .unwrap();
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    let f = report.features.iter().find(|f| f.id == lf).unwrap();
    let text = format!("{:?}", f.status);
    assert!(text.contains("同じ平面にあります"), "{text}");
    // 同じ断面が 2 度入る・断面が多すぎる時は、カーネルに渡す前に理由つきで止まる
    for (sections, want) in [
        (
            vec![section(s.square), section(s.circle), section(s.square)],
            "断面 1 と 3 が同じ",
        ),
        (
            vec![section(s.square); MAX_SECTIONS + 1],
            "個までにしてください",
        ),
    ] {
        let bad = Loft {
            sections,
            ..bad.clone()
        };
        doc.apply(Transaction::new(
            "直す",
            vec![Op::SetFeatureData {
                id: lf,
                data: serde_json::to_value(&bad).unwrap(),
            }],
        ))
        .unwrap();
        let report = doc.recompute(&reg, &*k);
        let f = report.features.iter().find(|f| f.id == lf).unwrap();
        let text = format!("{:?}", f.status);
        assert!(text.contains(want), "{text}");
    }
    let _ = assert_ok;
}

/// 穴のある断面(20 × 20 の四角に直径 8 の円)を下と 30 mm 上に描くと、穴の通った中空のロフトになる。
#[test]
fn a_loft_of_sections_with_holes_stays_hollow() {
    let k = TestKernel::new();
    if k.is_truck() {
        jc_contract::skip!("truck はロフトを作れない");
    }
    let mut b = DocBuilder::new();
    let mut section = |name: &str, offset: Option<&str>| {
        let mut s = SketchBuilder::new(jc_sketch::SketchPlane::Xy);
        let corner = s.fixed_point(-10.0, -10.0);
        s.rectangle(corner, "20 mm", "20 mm", [-10.0, -10.0, 20.0, 20.0]);
        let c = s.fixed_point(0.0, 0.0);
        s.circle(c, "8 mm", 4.0);
        let mut sk = s.build();
        sk.offset = offset.map(str::to_string);
        b.feature(jc_sketch::SKETCH, name, sk)
    };
    let low = section("下", None);
    let high = section("上", Some("30 mm"));
    let mut doc = b.finish("中空のロフト");
    let section = |profile| Section {
        profile,
        regions: vec![],
    };
    // 領域を選ばない断面は、四角から円を抜いた領域(穴のある輪郭)
    let data = Loft {
        sections: vec![section(low), section(high)],
        rails: vec![],
        operation: Operation::New,
        target: None,
    };
    add(&mut doc, LOFT, "ロフト1", serde_json::to_value(&data).unwrap());
    let m = final_mass(&mut doc, &k);
    let want = 400.0 * 30.0 - PI * 16.0 * 30.0;
    assert!(close(m.volume, want, 1e-4), "{} {want}", m.volume);
}
