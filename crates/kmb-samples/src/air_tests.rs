//! 加工ごとの高さ(安全・送り・トップ・ボトム)と、前に削った所の空振りを省くこと。

use crate::reference_cam;
use kmb_cam_commands::*;
use kmb_core::{FeatureId, FeatureStatus};
use kmb_toolpath::Move;

thread_local! {
    /// 同じ文書を別のカーネルで計算し直さない
    static MODULES: kmb_core::Modules = kmb_bundle::modules();
}

fn modules() -> kmb_core::Modules {
    MODULES.with(|m| m.clone())
}

fn insert_op(
    cam: &mut crate::ReferenceCam,
    type_id: &str,
    name: &str,
    data: serde_json::Value,
) -> FeatureId {
    let id = cam.part.doc.alloc_feature_id();
    cam.part
        .doc
        .apply(kmb_core::Transaction::new(
            name,
            vec![kmb_core::Op::InsertFeature {
                index: None,
                entry: kmb_core::FeatureEntry {
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

fn set(cam: &mut crate::ReferenceCam, id: FeatureId, key: &str, v: serde_json::Value) {
    let mut data = cam.part.doc.data().feature(id).unwrap().data.clone();
    data[key] = v;
    cam.part
        .doc
        .apply(kmb_core::Transaction::new(
            "設定",
            vec![kmb_core::Op::SetFeatureData { id, data }],
        ))
        .unwrap();
}

/// 送りの動き(直線と円弧)の長さ。
fn feed_length(tp: &Toolpath) -> f64 {
    let mut cur: Option<[f64; 3]> = None;
    let mut len = 0.0;
    for m in &tp.moves {
        if let (Some(from), Move::Linear { .. } | Move::Arc { .. }) = (cur, m) {
            let mut a = from;
            for q in m.points(from, 0.2) {
                len +=
                    ((q[0] - a[0]).powi(2) + (q[1] - a[1]).powi(2) + (q[2] - a[2]).powi(2)).sqrt();
                a = q;
            }
        }
        cur = Some(m.to());
    }
    len
}

fn index_of(s: &SetupPaths, name: &str) -> usize {
    s.paths.iter().position(|p| p.name == name).unwrap()
}

/// 同じ所をもう一度ポケットで削ると、2 つ目はほとんど空振りなので省かれる。省いても
/// 削れる形は同じで、材料へ早送りで入らない。欄を切ると省かない。
#[test]
fn a_second_pocket_over_the_same_spot_skips_what_the_first_one_cut() {
    let m = modules();
    let mut cam = reference_cam();
    let data = cam
        .part
        .doc
        .data()
        .feature(cam.pocket)
        .unwrap()
        .data
        .clone();
    let again = insert_op(&mut cam, kmb_strategy_pocket2d::POCKET, "もう一度", data);
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let s = setups(cam.part.doc.data(), &report, &m.registry).remove(0);
    let k = index_of(&s, "もう一度");
    assert!(s.sources[k].skip_air);
    assert!(s.sources[k].original.is_some(), "省かない");
    let (before, after) = (feed_length(s.generated(k)), feed_length(&s.paths[k]));
    println!("送りの長さ {before:.1} → {after:.1} mm");
    // 壁に沿う仕上げは、前の壁に触れたままなので残る
    assert!(after < 0.4 * before, "{before} → {after}");
    assert!(
        kmb_toolpath::check(&s.paths[k]).is_empty(),
        "{:?}",
        kmb_toolpath::check(&s.paths[k])
    );
    // 省いた区間はつなぎとして記録される
    assert!(s.paths[k].links.links.len() > s.generated(k).links.links.len() / 2);
    let kernel = m.kernel.as_ref();
    let trimmed = simulate(kernel, &s, SimOptions::default()).unwrap();
    assert!(trimmed.is_ok(), "{}", trimmed.summary());

    // 欄を切ると、元の経路のまま
    set(&mut cam, again, "air", serde_json::json!("keep"));
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let full = setups(cam.part.doc.data(), &report, &m.registry).remove(0);
    let k = index_of(&full, "もう一度");
    assert!(full.sources[k].original.is_none());
    assert!((feed_length(&full.paths[k]) - before).abs() < 1e-6);
    let plain = simulate(kernel, &full, SimOptions::default()).unwrap();
    assert!(plain.is_ok(), "{}", plain.summary());
    // 削れる形は(格子の粗さの内で)同じ
    let d = (trimmed.removed_volume - plain.removed_volume).abs();
    assert!(
        d < 0.005 * plain.removed_volume,
        "{} {}",
        trimmed.removed_volume,
        plain.removed_volume
    );
}

/// 加工を選んで書き出す・工具ごとに並べ替えると、その順で省き直す(前の加工を外すと、
/// その加工が削るはずだった所は空振りでなくなる)。
#[test]
fn choosing_only_some_operations_redoes_the_skipping() {
    let m = modules();
    let mut cam = reference_cam();
    let data = cam
        .part
        .doc
        .data()
        .feature(cam.pocket)
        .unwrap()
        .data
        .clone();
    let again = insert_op(&mut cam, kmb_strategy_pocket2d::POCKET, "もう一度", data);
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let s = setups(cam.part.doc.data(), &report, &m.registry).remove(0);
    let k = index_of(&s, "もう一度");
    let trimmed = feed_length(&s.paths[k]);
    let only = s.only(&[again]).unwrap();
    assert_eq!(only.paths.len(), 1);
    let alone = feed_length(&only.paths[0]);
    assert!(alone > 3.0 * trimmed, "{trimmed} {alone}");
    let (by_tool, ..) = s.by_tool().unwrap();
    let k = index_of(&by_tool, "もう一度");
    assert!(by_tool.sources[k].original.is_some());
}

/// 次のセットアップは、前のセットアップの削り跡から始める。同じ向きなら同じポケットは
/// ほとんど空振りで、裏返しても、表で貫通させた所は空いている(表のポケットをストックの
/// 底面の下まで深くし、裏の上面を薄くフェイスで削る)。
#[test]
fn the_next_setup_skips_what_the_setups_before_it_cut() {
    let m = modules();
    for flip in ["none", "around_y"] {
        let mut cam = reference_cam();
        let pocket = cam.pocket;
        set(
            &mut cam,
            pocket,
            "bottom_from",
            serde_json::json!("stock_bottom"),
        );
        set(
            &mut cam,
            pocket,
            "bottom_offset",
            serde_json::json!("-0.3 mm"),
        );
        let mut data = cam.part.doc.data().feature(cam.setup).unwrap().data.clone();
        data["flip"] = serde_json::json!(flip);
        if flip != "none" {
            // 裏返すと上と下の余白が入れ替わる(同じストックの塊にする)
            let stock = &mut data["stock"];
            let (t, b) = (stock["margin_top"].clone(), stock["margin_bottom"].clone());
            stock["margin_top"] = b;
            stock["margin_bottom"] = t;
        }
        let next = insert_op(&mut cam, SETUP, "次", data);
        let (src, name) = if flip == "none" {
            (pocket, "次のポケット")
        } else {
            (cam.face, "裏のフェイス")
        };
        let mut p = cam.part.doc.data().feature(src).unwrap().data.clone();
        p["setup"] = serde_json::json!(next);
        if flip != "none" {
            p["bottom_from"] = serde_json::json!("stock_top");
            p["bottom_offset"] = serde_json::json!("-0.5 mm");
        }
        let type_id = cam.part.doc.data().feature(src).unwrap().type_id.clone();
        insert_op(&mut cam, &type_id, name, p);
        let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
        assert_eq!(report.error_count(), 0, "{flip}: {:?}", report.features);
        let all = setups(cam.part.doc.data(), &report, &m.registry);
        let s = all.iter().find(|s| s.setup == next).unwrap();
        assert!(s.air_start.is_some(), "{flip}");
        let k = index_of(s, name);
        assert!(
            s.sources[k].original.is_some(),
            "{flip}: 前の削り跡を見ていない"
        );
        let (before, after) = (feed_length(s.generated(k)), feed_length(&s.paths[k]));
        println!("{flip}: 送りの長さ {before:.1} → {after:.1} mm");
        if flip == "none" {
            assert!(after < 0.5 * before, "{flip}: {before} → {after}");
        } else {
            // 裏の上面で空いているのは、貫通したポケットの所だけ
            assert!(after < before - 5.0, "{flip}: {before} → {after}");
        }
        assert!(kmb_toolpath::check(&s.paths[k]).is_empty(), "{flip}");
        // 次のセットアップを前の削り跡から削っても、早送りで材料に当たらない
        let i = all.iter().position(|s| s.setup == next).unwrap();
        let sim = simulate_chain(m.kernel.as_ref(), &all, i, SimOptions::default()).unwrap();
        assert!(sim.rapid_cuts.is_empty(), "{flip}: {}", sim.summary());
    }
}

fn pocket_heights(cam: &mut crate::ReferenceCam) -> Result<(Toolpath, HeightsOutput), String> {
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    match report.feature(cam.pocket) {
        Some(FeatureStatus::Ok { outputs, .. }) => {
            let h = outputs
                .get("heights")
                .and_then(heights_of)
                .expect("高さの出力");
            let s = setups(cam.part.doc.data(), &report, &m.registry).remove(0);
            let name = cam
                .part
                .doc
                .data()
                .feature(cam.pocket)
                .unwrap()
                .name
                .clone();
            Ok((s.generated(index_of(&s, &name)).clone(), h))
        }
        Some(FeatureStatus::Error { message }) => Err(message.clone()),
        other => panic!("{other:?}"),
    }
}

fn levels(tp: &Toolpath) -> (f64, f64) {
    let zs: Vec<f64> = tp
        .moves
        .iter()
        .filter(|m| !matches!(m, Move::Rapid { .. }))
        .map(|m| m.to()[2])
        .collect();
    (
        zs.iter().copied().fold(f64::MIN, f64::max),
        zs.iter().copied().fold(f64::MAX, f64::min),
    )
}

/// トップ・ボトム高さのオフセットで削る範囲が変わり、送り・安全高さを材料の中や送り高さの
/// 下へ置くと誤りになる。
#[test]
fn the_heights_of_an_operation_move_where_it_cuts() {
    let mut cam = reference_cam();
    let pocket = cam.pocket;
    let (tp, h) = pocket_heights(&mut cam).unwrap();
    assert!(
        h.safe >= h.feed && h.feed > h.top.unwrap() && h.top > h.bottom,
        "{h:?}"
    );
    let (top0, bottom0) = levels(&tp);
    // トップを 1 mm 下げると、上の段がその分下から始まる
    set(&mut cam, pocket, "top_offset", serde_json::json!("-1 mm"));
    let (tp, h1) = pocket_heights(&mut cam).unwrap();
    assert!(
        (h1.top.unwrap() - (h.top.unwrap() - 1.0)).abs() < 1e-9,
        "{h1:?}"
    );
    // 送り高さはトップの上の同じ余裕
    assert!((h1.feed - (h.feed - 1.0)).abs() < 1e-9, "{h1:?}");
    let (top1, bottom1) = levels(&tp);
    assert!(
        top1 < top0 - 1e-6 && top1 <= h1.top.unwrap(),
        "{top0} {top1}"
    );
    assert!((bottom1 - bottom0).abs() < 1e-9);
    // ボトムを 1 mm 上げると、床がその分浅い
    set(&mut cam, pocket, "top_offset", serde_json::Value::Null);
    set(&mut cam, pocket, "bottom_offset", serde_json::json!("1 mm"));
    let (tp, _) = pocket_heights(&mut cam).unwrap();
    assert!((levels(&tp).1 - (bottom0 + 1.0)).abs() < 1e-6, "{bottom0}");
    // 送り高さをストックの底面にすると、材料の中へ早送りで下りるので誤り
    set(&mut cam, pocket, "bottom_offset", serde_json::Value::Null);
    set(
        &mut cam,
        pocket,
        "feed_from",
        serde_json::json!("stock_bottom"),
    );
    let e = pocket_heights(&mut cam).unwrap_err();
    assert!(e.contains("送り高さ"), "{e}");
    // 安全高さを送り高さの下にすると誤り
    set(&mut cam, pocket, "feed_from", serde_json::json!("auto"));
    set(&mut cam, pocket, "safe_from", serde_json::json!("top"));
    let e = pocket_heights(&mut cam).unwrap_err();
    assert!(e.contains("安全高さ"), "{e}");
    // 安全高さを上げると、経路の安全高さも上がる
    set(&mut cam, pocket, "safe_from", serde_json::json!("auto"));
    set(&mut cam, pocket, "safe_offset", serde_json::json!("5 mm"));
    let (tp, h2) = pocket_heights(&mut cam).unwrap();
    assert!((h2.safe - (h.safe + 5.0)).abs() < 1e-9, "{h2:?}");
    assert!((tp.safe_z - h2.safe).abs() < 1e-9);
    assert!(
        kmb_toolpath::check(&tp).is_empty(),
        "{:?}",
        kmb_toolpath::check(&tp)
    );
}

/// 加工を選ぶと、3D の画面にその加工の 4 つの高さの面が出る。セットアップを選ぶと出ない。
#[test]
fn selecting_an_operation_shows_its_height_planes() {
    let m = modules();
    let mut cam = reference_cam();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = kmb_core::Context {
        selected: Some(cam.pocket),
        ..kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    let o = m.extensions.overlay(&cx);
    let inks: Vec<_> = o.planes.iter().map(|p| p.ink).collect();
    assert_eq!(
        inks,
        [
            kmb_interchange::ink::plane::SAFE,
            kmb_interchange::ink::plane::FEED,
            kmb_interchange::ink::plane::TOP,
            kmb_interchange::ink::plane::BOTTOM
        ]
    );
    assert!(
        o.planes[0].label.starts_with("安全高さ Z"),
        "{}",
        o.planes[0].label
    );
    // 面は上から順に並ぶ
    let z: Vec<f64> = o.planes.iter().map(|p| p.corners[0][2]).collect();
    assert!(z.windows(2).all(|w| w[0] > w[1]), "{z:?}");
    let cx = kmb_core::Context {
        selected: Some(cam.setup),
        ..kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    assert!(m.extensions.overlay(&cx).planes.is_empty());
}

/// G-code の頭の材料は、削るボディに付けた材料(部品表と同じ)を先に使い、付けていない時だけ
/// 設計の属性の材料を書く。
#[test]
fn the_gcode_header_names_the_body_material_before_the_property() {
    let m = modules();
    let mut cam = reference_cam();
    let mut props = cam.part.doc.data().properties.clone();
    props.set("material", "A5052");
    let set_props = kmb_core::Op::SetProperties {
        properties: Box::new(props),
    };
    cam.part
        .doc
        .apply(kmb_core::Transaction::new("属性", vec![set_props]))
        .unwrap();
    let material_line = |cam: &mut crate::ReferenceCam| {
        let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
        let s = setups(cam.part.doc.data(), &report, &m.registry).remove(0);
        s.part.into_iter().find(|l| l.starts_with("材料 "))
    };
    assert_eq!(material_line(&mut cam).as_deref(), Some("材料 A5052"));
    let data = cam.part.doc.data();
    let root = kmb_core::body_root(data, &m.registry, cam.setup);
    let mut mats = kmb_look::material::of(data);
    mats.set(root, Some("アルミ"));
    let op = data.section_op(&mats).unwrap();
    cam.part
        .doc
        .apply(kmb_core::Transaction::new("材料", vec![op]))
        .unwrap();
    assert_eq!(material_line(&mut cam).as_deref(), Some("材料 アルミ"));
}
