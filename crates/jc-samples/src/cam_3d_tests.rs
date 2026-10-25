//! 3D の加工(走査線・等高線・ペンシル・荒削り・投影・らせん・スカラップ)を 3D の見本で確かめる。

use crate::cam_tests::*;
use crate::{reference_cam, reference_cam_3d};
use jc_cam_commands::*;
use jc_core::FeatureStatus;
use jc_post_carvera_air::CarveraAir;
use jc_strategy_adaptive2d::ADAPTIVE;
use jc_strategy_face::FACE;
use jc_strategy_pencil::{PENCIL, Pencil};
use jc_strategy_pocket2d::POCKET;
use jc_strategy_raster3d::{
    Circular, MORPH3D, Morph, PROJECT3D, Pattern as CircularPattern, Project, SCALLOP3D, SPIRAL3D,
    Scallop,
};
use jc_strategy_rough3d::{FLAT3D, Flat3d, ROUGH3D, Rough3d};
use jc_strategy_waterline::{RAMP3D, Ramp3d, WATERLINE, Waterline};

// 3D の見本を 1 回だけ計算し、既定のシミュレーションも 1 回にして、次をまとめて確かめる(文書を
// 変える段は取り消して戻し、つなぎを変える段は最後に置く):
// - 3D の見本はボールエンドミルで仕上げて食い込まない。等高線は段が多く、上げる回数が少なく、急に下りない
// - 3D の経路を円弧にまとめると G-code が 1 割以上減り、削れる形は同じで、時間の目安も短い
// - 走査線と等高線は、入る位置を置くとその近くから始める
// - つなぎを下げたまま・低い退避にすると全退避が減り、経路でも G-code でも食い込まず形は同じ
#[test]
fn the_3d_sample_is_finished_with_a_ball_endmill_and_its_path_options_keep_the_shape() {
    let mut cam = reference_cam_3d();
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    // 手を加えない文書の経路と、その既定のシミュレーション(後の段でも比べる基準にする)
    let base = recompute_ok(&mut cam, &reg, kernel);
    let start = cam.part.doc.undo_depth();
    let opt = SimOptions::default();
    let base_sim = simulate(kernel, &base, opt).unwrap();

    // --- ボールエンドミルで仕上げる
    let s = &base;
    assert_eq!(s.paths.len(), 8);
    let tools: Vec<u32> = s.program().tools().iter().map(|t| t.number).collect();
    assert_eq!(tools, vec![1, 2, 4]);
    let g = gcode(s, &CarveraAir, &jc_post_carvera_air::profile()).unwrap();
    assert!(g.contains("M6 T4"));
    let sim = &base_sim;
    println!("{}", sim.summary());
    for (name, v) in &sim.removed_by_op {
        println!("  {name}: {v:.1} mm³");
    }
    assert!(sim.is_ok(), "{}", sim.summary());
    let raster = sim
        .removed_by_op
        .iter()
        .find(|(n, _)| n == "面取りと角の仕上げ")
        .map(|(_, v)| *v)
        .unwrap();
    // 面取り(1 mm)の三角柱は外周と上面のポケットの縁で約 0.5 × (200 + 54) mm³。
    // ボールで削れるのはその大半(平らな底の際はボールの丸みの分だけ残る)
    assert!(raster > 60.0, "走査線で削った体積 {raster}");
    // 等高線は、走査線が段に残した縦の壁の際と、ボスの周りを回る
    let water = &s.paths[7];
    assert_eq!(water.name, "縦の壁の仕上げ");
    // 段は、高さを変えずに送る動きの高さ(輪に沿って下りる所は数えない)
    let levels: std::collections::BTreeSet<i64> = water
        .moves
        .windows(2)
        .filter(|w| {
            let (a, b) = (w[0].to(), w[1].to());
            matches!(w[1], jc_toolpath::Move::Linear { .. })
                && (b[2] - a[2]).abs() < 1e-9
                && (b[0] - a[0]).hypot(b[1] - a[1]) > 1e-6
        })
        .map(|w| (w[1].to()[2] * 1000.0).round() as i64)
        .collect();
    assert!(levels.len() >= 10, "段の数 {}", levels.len());
    // 輪から輪へは、近くて削らずに動ければ上げずに移る。下の段へは輪に沿って 3 deg で下りる
    let lifts = water
        .moves
        .iter()
        .filter(|m| matches!(m, jc_toolpath::Move::Rapid { to } if to[2] >= water.safe_z - 1e-9))
        .count();
    let mut cur = water.moves[0].to();
    let mut steep = 0;
    for m in &water.moves[1..] {
        let to = m.to();
        let run = (to[0] - cur[0]).hypot(to[1] - cur[1]);
        if matches!(m, jc_toolpath::Move::Linear { .. })
            && to[2] < cur[2] - 1e-9
            && run > 1e-9
            && (cur[2] - to[2]) / run > 3.01f64.to_radians().tan()
        {
            steep += 1;
        }
        cur = to;
    }
    println!("等高線: 上げた回数 {lifts}、段の数 {}", levels.len());
    assert_eq!(steep, 0, "3 deg より急に斜めに下りている");
    // 輪ごとに上げると 50 回(2026-10-13 の実測)。平らな床が段の高さにある所でも、
    // 床の上に細かな輪ができない(できていた時は 3000 回を超えた)。四隅の穴(径 3.4 mm)には
    // 径 3.175 mm のボールが入るので、どの段でも穴の壁を回る(2026-10-21 に三角形へ直接下ろす
    // ようにしてから。高さの格子では外へ 0.1 mm ほどずれて、-7 mm より下で輪が消えていた)。
    // 穴ごと段ごとに上げる分が足されて 70 回
    assert!(lifts <= 80, "上げた回数 {lifts}");

    // --- 走査線と等高線を円弧にまとめる(基準と比べ、後で取り消す)
    for id in [cam.raster.unwrap(), cam.waterline.unwrap()] {
        let mut d = cam.part.doc.data().feature(id).unwrap().data.clone();
        d["fit"] = serde_json::json!("0.005 mm");
        set_data(&mut cam.part.doc, id, d);
    }
    let fitted = recompute_ok(&mut cam, &reg, kernel);
    let machine = jc_post_carvera_air::profile();
    let lines = |s: &SetupPaths| gcode(s, &CarveraAir, &machine).unwrap().lines().count();
    let moves =
        |s: &SetupPaths, name: &str| s.paths.iter().find(|p| p.name == name).unwrap().moves.len();
    for name in ["面取りと角の仕上げ", "縦の壁の仕上げ"] {
        println!(
            "{name}: 動き {} → {}",
            moves(&base, name),
            moves(&fitted, name)
        );
        assert!(moves(&fitted, name) < moves(&base, name), "{name}");
    }
    let (a, b) = (lines(&base), lines(&fitted));
    println!("G-code の行 {a} → {b}");
    assert!(b * 10 < a * 9, "1 割以上減る: {a} → {b}");
    let sa = &base_sim;
    let sb = simulate(kernel, &fitted, opt).unwrap();
    assert!(sb.is_ok(), "{}", sb.summary());
    let differ = sa
        .result
        .z
        .iter()
        .zip(&sb.result.z)
        .filter(|(p, q)| (*p - *q).abs() > 0.02)
        .count() as f64
        / sa.result.z.len() as f64;
    println!("形の違うセル {:.4}%", differ * 100.0);
    assert!(differ < 0.002, "{differ}");
    // 時間の目安も短くなる(細かい継ぎ目ごとに速さを落とさない)
    let m = jc_toolpath::Motion {
        rapid_feed: 3000.0,
        acceleration: 150.0,
        junction_deviation: 0.05,
        max_rate: [3000.0, 3000.0, 2000.0],
    };
    // 円弧にまとめても、時間の目安はほとんど変わらない。等高線の輪が格子の幅でぎざぎざだった間は
    // ぎざぎざの角が消えて短くなっていたが、輪が滑らかになった今は、四隅の穴(径 3.4 mm に
    // 径 3.175 mm のボール)を回る周 0.7 mm ほどの小さな輪の角で、短い区間が消えて 2 つの角が
    // 1 つの鋭い角になり、そこで速さを落とす分だけわずかに長くなる(32.48 → 32.56 分、0.23%。
    // 2026-10-21 の実測)
    let (before, after) = (base.program().minutes(&m), fitted.program().minutes(&m));
    println!("時間の目安 {before:.2} → {after:.2} 分");
    assert!(after < before * 1.005, "{before} → {after}");
    cam.part.doc.rollback_to(start);

    // --- 入る位置を置くと、走査線と等高線がその近くから始める(後で取り消す)
    let (raster, waterline) = (cam.raster.unwrap(), cam.waterline.unwrap());
    let s = &base;
    let (rn, wn) = (
        cam.part.doc.data().feature(raster).unwrap().name.clone(),
        cam.part.doc.data().feature(waterline).unwrap().name.clone(),
    );
    let name = |id| if id == raster { rn.clone() } else { wn.clone() };
    let first_cut = |s: &SetupPaths, id| {
        let n = name(id);
        let tp = s.generated(s.paths.iter().position(|p| p.name == n).unwrap());
        tp.moves
            .iter()
            .find(|m| !matches!(m, jc_toolpath::Move::Rapid { .. }))
            .unwrap()
            .to()
    };
    let n = name(raster);
    let tp = s.generated(s.paths.iter().position(|p| p.name == n).unwrap());
    let last = tp
        .moves
        .iter()
        .rev()
        .find(|m| !matches!(m, jc_toolpath::Move::Rapid { .. }))
        .unwrap()
        .to();
    let before = first_cut(s, raster);
    // 往復の終わりの側に入る位置を置くと、そちらから始める
    let mut d = feature_data(&cam, raster);
    d["entry"] = serde_json::json!([last[0], last[1]]);
    set_data(&mut cam.part.doc, raster, d);
    // 等高線は右奥の角に置く
    let mut d = feature_data(&cam, waterline);
    d["entry"] = serde_json::json!([100.0, 100.0]);
    set_data(&mut cam.part.doc, waterline, d);
    let s = recompute_ok(&mut cam, &reg, kernel);
    let after = first_cut(&s, raster);
    assert!(
        (after[0] - last[0]).hypot(after[1] - last[1])
            < (before[0] - last[0]).hypot(before[1] - last[1]),
        "{before:?} → {after:?}(入る位置 {last:?})"
    );
    let w = first_cut(&s, waterline);
    let mid = [
        (s.info.stock_min[0] + s.info.stock_max[0]) / 2.0,
        (s.info.stock_min[1] + s.info.stock_max[1]) / 2.0,
    ];
    assert!(w[0] > mid[0] && w[1] > mid[1], "{w:?}");
    cam.part.doc.rollback_to(start);

    // --- つなぎを元のまま(下げたまま移らない、いつも安全高さ)にした文書と比べる(最後の段)
    let linked = &base;
    let ops: Vec<_> = cam
        .part
        .doc
        .data()
        .features
        .iter()
        .filter(|f| f.data.get("tools").is_some() && f.data.get("tool").is_some())
        .map(|f| f.id)
        .collect();
    for &id in &ops {
        let mut d = cam.part.doc.data().feature(id).unwrap().data.clone();
        d["stay_down"] = serde_json::json!("0 mm");
        d["retract"] = serde_json::json!("safe");
        set_data(&mut cam.part.doc, id, d);
    }
    let plain = recompute_ok(&mut cam, &reg, kernel);
    let machine = jc_post_carvera_air::profile();
    let motion = machine.motion();
    use jc_toolpath::LinkKind;
    let (mut full_a, mut full_b, mut air_a, mut air_b) = (0, 0, 0.0, 0.0);
    for (a, b) in plain.paths.iter().zip(&linked.paths) {
        let n = |p: &jc_toolpath::Toolpath, k| p.links.count(k);
        println!(
            "{}: 全退避 {} → {}(下げたまま {}、持ち上げ {}、低い退避 {})、空の動き {:.2} → {:.2} 分、時間 {:.2} → {:.2} 分",
            a.name,
            n(a, LinkKind::Full),
            n(b, LinkKind::Full),
            n(b, LinkKind::StayDown),
            n(b, LinkKind::Lift),
            n(b, LinkKind::Low),
            a.air_minutes(&motion),
            b.air_minutes(&motion),
            a.minutes(&motion),
            b.minutes(&motion)
        );
        assert!(jc_toolpath::check(b).is_empty(), "{}", b.name);
        // つなぎの外の動きが同じ時だけ、加工ごとに比べる(等高線の輪の順は、計算のたびに
        // 変わることがある)
        let outside = |p: &jc_toolpath::Toolpath| -> Vec<jc_toolpath::Move> {
            p.moves
                .iter()
                .enumerate()
                .filter(|(i, _)| p.links.link_at(*i).is_none())
                .map(|(_, m)| *m)
                .collect()
        };
        if outside(a) == outside(b) {
            assert!(n(b, LinkKind::Full) <= n(a, LinkKind::Full), "{}", a.name);
            assert!(
                b.minutes(&motion) <= a.minutes(&motion) + 1e-9,
                "{}",
                a.name
            );
        } else {
            println!("  ({} はつなぎの外の動きが違うので比べない)", a.name);
        }
        full_a += n(a, LinkKind::Full);
        full_b += n(b, LinkKind::Full);
        air_a += a.air_minutes(&motion);
        air_b += b.air_minutes(&motion);
    }
    println!(
        "合計: 全退避 {full_a} → {full_b}、空の動き {air_a:.2} → {air_b:.2} 分、時間 {:.2} → {:.2} 分",
        plain.program().minutes(&motion),
        linked.program().minutes(&motion)
    );
    assert!(full_b < full_a, "{full_a} → {full_b}");
    // 経路でも、Carvera Air のポストで書いた G-code を読み直しても、食い込み・早送りでの
    // 切削・シャンクとホルダーの当たりはなく、削れる形は同じ
    let sa = simulate(kernel, &plain, opt).unwrap();
    let sb = &base_sim;
    println!("{}", sb.summary());
    assert!(sb.is_ok(), "{}", sb.summary());
    let g = gcode(linked, &CarveraAir, &machine).unwrap();
    let from_g = with_gcode_paths(linked, &g).unwrap();
    let sg = simulate(kernel, &from_g, opt).unwrap();
    assert!(sg.is_ok(), "{}", sg.summary());
    for (x, what) in [(&sb.result, "経路"), (&sg.result, "G-code")] {
        let differ = sa
            .result
            .z
            .iter()
            .zip(&x.z)
            .filter(|(p, q)| (*p - *q).abs() > 0.02)
            .count() as f64
            / sa.result.z.len() as f64;
        println!("{what}: 形の違うセル {:.4}%", differ * 100.0);
        assert!(differ < 0.002, "{what} {differ}");
    }
}

#[test]
fn a_3d_raster_runs_at_the_given_angle_without_gouging() {
    let mut cam = reference_cam_3d();
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let raster = cam.raster.unwrap();
    let mut data = cam.part.doc.data().feature(raster).unwrap().data.clone();
    data["angle"] = serde_json::json!("30 deg");
    cam.part
        .doc
        .apply(jc_core::Transaction::new(
            "往復の角度",
            vec![jc_core::Op::SetFeatureData { id: raster, data }],
        ))
        .unwrap();
    let report = cam.part.doc.recompute(&reg, kernel);
    for f in &report.features {
        assert!(
            matches!(f.status, FeatureStatus::Ok { .. }),
            "{} {}: {:?}",
            f.id,
            f.name,
            f.status
        );
    }
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let path = s
        .paths
        .iter()
        .find(|p| p.name == "面取りと角の仕上げ")
        .unwrap();
    // 送りの長さを向きごとに足す(0〜180 度、10 度ごと)。いちばん長いのは 30 度の向き
    let mut by_angle = [0.0f64; 18];
    let mut cur = path.moves[0].to();
    for m in &path.moves[1..] {
        let to = m.to();
        let (dx, dy) = (to[0] - cur[0], to[1] - cur[1]);
        if matches!(m, jc_toolpath::Move::Linear { .. }) && dx.hypot(dy) > 1e-9 {
            let a = dy.atan2(dx).to_degrees().rem_euclid(180.0);
            by_angle[((a + 5.0) / 10.0) as usize % 18] += dx.hypot(dy);
        }
        cur = to;
    }
    let total: f64 = by_angle.iter().sum();
    assert!(by_angle[3] > 0.8 * total, "{by_angle:?}");
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    println!("{}", sim.summary());
    assert!(sim.is_ok(), "{}", sim.summary());
    let removed = sim
        .removed_by_op
        .iter()
        .find(|(n, _)| n == "面取りと角の仕上げ")
        .map(|(_, v)| *v)
        .unwrap();
    // X の向きと同じく、面取りの大半を削る
    assert!(removed > 60.0, "走査線で削った体積 {removed}");
}

fn pencil(cam: &crate::ReferenceCam, tool: u32) -> Pencil {
    Pencil {
        base: OpBase {
            fit: Some("0.005 mm".into()),
            ..OpBase::new(cam.setup, cam.tools, tool)
        },
        leave: "0 mm".into(),
        resolution: "0.05 mm".into(),
        angle: None,
        rest_tool: None,
        depth: None,
        passes: 0,
        pitch: None,
        limits: Default::default(),
    }
}

#[test]
fn a_pencil_traces_the_inside_corners_after_the_raster_without_gouging() {
    let mut cam = reference_cam_3d();
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let data = json(&pencil(&cam, 4));
    insert_op(&mut cam, PENCIL, "内側の角", data);
    let started = std::time::Instant::now();
    let s = recompute_ok(&mut cam, &reg, kernel);
    println!("計算 {:.1} 秒", started.elapsed().as_secs_f64());
    let path = s.paths.iter().find(|p| p.name == "内側の角").unwrap();
    let (cut, _) = path.lengths();
    let arcs = path
        .moves
        .iter()
        .filter(|m| matches!(m, jc_toolpath::Move::Arc { .. }))
        .count();
    println!(
        "ペンシル: 動き {}、円弧 {arcs}、送りの長さ {cut:.0} mm",
        path.moves.len()
    );
    let info = &s.info;
    // なぞるのはストックの上面より下、部品の底より上
    for m in &path.moves {
        let q = m.to();
        if matches!(m, jc_toolpath::Move::Rapid { .. }) {
            continue;
        }
        assert!(
            q[2] < info.stock_max[2] && q[2] > info.stock_min[2] - 1e-6,
            "{q:?}"
        );
    }
    // 線ごとの範囲(下ろしてから上げるまで)
    let mut run: Vec<[f64; 3]> = Vec::new();
    for m in &path.moves {
        if matches!(m, jc_toolpath::Move::Rapid { .. }) {
            if run.len() > 1 {
                let lo = run.iter().fold([f64::INFINITY; 3], |a, q| {
                    [a[0].min(q[0]), a[1].min(q[1]), a[2].min(q[2])]
                });
                let hi = run.iter().fold([f64::NEG_INFINITY; 3], |a, q| {
                    [a[0].max(q[0]), a[1].max(q[1]), a[2].max(q[2])]
                });
                println!("線: {} 点 {lo:.2?} 〜 {hi:.2?}", run.len());
            }
            run.clear();
        } else {
            run.push(m.to());
        }
    }
    assert!(arcs > 0, "内側の丸い角は円弧になる");
    let sim = simulate(kernel, &s, SimOptions::default()).unwrap();
    println!("{}", sim.summary());
    for (name, v) in &sim.removed_by_op {
        println!("  {name}: {v:.2} mm³");
    }
    assert!(sim.is_ok(), "{}", sim.summary());
    let g = gcode(&s, &CarveraAir, &jc_post_carvera_air::profile()).unwrap();
    let i = g.find("(内側の角)").unwrap();
    assert!(
        g[i..]
            .lines()
            .any(|l| l.starts_with("G2") || l.starts_with("G3"))
    );
}

#[test]
fn a_pencil_after_a_thicker_ball_cuts_only_what_it_left() {
    let mut cam = reference_cam_3d();
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    // 細いボール(1 mm)を足し、前の工具を 3.175 mm のボールにする
    add_tool(
        &mut cam,
        Tool {
            number: 9,
            name: "ボール 1 mm".into(),
            diameter: 1.0,
            flute_length: 12.0,
            ..ToolLibrary::starter().tools[3].clone()
        },
    );
    let all = json(&pencil(&cam, 9));
    let id = insert_op(&mut cam, PENCIL, "内側の角", all);
    let s = recompute_ok(&mut cam, &reg, kernel);
    let length_of = |s: &SetupPaths| {
        s.paths
            .iter()
            .find(|p| p.name == "内側の角")
            .unwrap()
            .lengths()
            .0
    };
    let every = length_of(&s);
    let mut rest = pencil(&cam, 9);
    rest.rest_tool = Some(4);
    set_data(&mut cam.part.doc, id, json(&rest));
    let s = recompute_ok(&mut cam, &reg, kernel);
    let only_left = length_of(&s);
    println!("谷をすべて {every:.0} mm、前のボールが残した所だけ {only_left:.0} mm");
    // 見本の谷はどれも丸みのない内側の角なので、3.175 mm のボールはどこも削り残す。残した所だけ
    // にしても、谷はほとんど全部なぞる(前の工具が届いた所を外すことは、ペンシルの単体の試験が
    // 同じ太さのボールで確かめる)。高さの格子で下ろしていた間は、格子のずれでできた偽の谷
    // (節点 2241 のうち 152)を前の工具が届いたとして外していたので 147 → 143 mm に減っていた。
    // 三角形へ直接下ろすと偽の谷ができず(節点 2134)、外れるのは 42 だけ(2026-10-21 の実測)
    assert!(
        only_left > 0.95 * every && only_left < 1.01 * every,
        "{only_left} {every}"
    );
    // 前の工具を同じ 1 mm のボールにすると、なぞる谷が残らない
    let mut same = pencil(&cam, 9);
    same.rest_tool = Some(9);
    set_data(&mut cam.part.doc, id, json(&same));
    let report = cam.part.doc.recompute(&reg, kernel);
    assert!(
        matches!(report.feature(id), Some(FeatureStatus::Error { message }) if message.contains("なぞる谷がありません")),
        "{:?}",
        report.feature(id)
    );
    set_data(&mut cam.part.doc, id, json(&rest));
    let s = recompute_ok(&mut cam, &reg, kernel);
    let sim = simulate(kernel, &s, SimOptions::default()).unwrap();
    println!("{}", sim.summary());
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn the_3d_rough_flat_and_spiral_ops_run_on_the_reference_part_without_gouging() {
    let mut cam = reference_cam_3d();
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let (setup, tools) = (cam.setup, cam.tools);
    let base = |tool| OpBase::new(setup, tools, tool);
    let rough = insert_op(&mut cam, ROUGH3D, "3D 粗取り", json(&Rough3d::new(base(1))));
    insert_op(&mut cam, FLAT3D, "平らな所", json(&Flat3d::new(base(1))));
    let spiral = Circular {
        depth: Some("6.5 mm".into()),
        ..Circular::new(base(4), CircularPattern::Spiral)
    };
    insert_op(&mut cam, SPIRAL3D, "渦巻き", json(&spiral));
    let started = std::time::Instant::now();
    let s = recompute_ok(&mut cam, &reg, kernel);
    println!("計算 {:.1} 秒", started.elapsed().as_secs_f64());
    for name in ["3D 粗取り", "平らな所", "渦巻き"] {
        let p = s.paths.iter().find(|p| p.name == name).unwrap();
        assert!(p.moves.len() > 10, "{name}: {}", p.moves.len());
    }
    let sim = simulate(kernel, &s, SimOptions::default()).unwrap();
    println!("{}", sim.summary());
    assert!(sim.is_ok(), "{}", sim.summary());
    // 入る位置を 3D 粗取りにも置ける
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let top = s.info.to_world([0.0, 0.0, s.info.stock_max[2]])[2];
    let args = serde_json::json!({"point": [1.0, 1.0, top]});
    let cx = jc_core::Context {
        selected: Some(rough),
        args: &args,
        ..jc_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    let Ok(jc_core::Outcome::Change { edits, .. }) = m.extensions.run(PLACE_ENTRY, &cx) else {
        panic!("入る位置を置けない");
    };
    let Some(jc_core::Edit::SetData { data, .. }) = edits.first() else {
        panic!()
    };
    assert!(data.get("entry").is_some(), "{data}");
    assert_eq!(data.get("leave"), Some(&serde_json::json!("0.3 mm")));
}

#[test]
fn projecting_the_top_sketch_follows_its_lines_on_the_part_without_gouging() {
    let mut cam = reference_cam_3d();
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let base = OpBase::new(cam.setup, cam.tools, 4);
    let sketch = cam.part.top_sketch;
    insert_op(
        &mut cam,
        PROJECT3D,
        "投影",
        json(&Project::new(base.clone(), sketch)),
    );
    // 面より 0.2 mm 下を彫る写し(彫った溝の分は食い込みとして数えない)
    let carve = Project {
        leave: "-0.2 mm".into(),
        ..Project::new(base, sketch)
    };
    insert_op(&mut cam, PROJECT3D, "投影で彫る", json(&carve));
    let s = recompute_ok(&mut cam, &reg, kernel);
    let path = |name: &str| s.paths.iter().find(|p| p.name == name).unwrap();
    // 上面のスケッチはポケットの四角(左下 8, 8 から 15 × 12)。工具の中心はその線の上を通るので、
    // 削る点の広がりは 15 × 12
    let cut: Vec<[f64; 3]> = path("投影")
        .moves
        .iter()
        .filter(|m| matches!(m, jc_toolpath::Move::Linear { .. }))
        .map(|m| m.to())
        .filter(|q| q[2] < s.info.stock_max[2] - 1e-6)
        .collect();
    let span = |k: usize| {
        let v = cut.iter().map(|q| q[k]);
        v.clone().fold(f64::NEG_INFINITY, f64::max) - v.fold(f64::INFINITY, f64::min)
    };
    assert!((span(0) - 15.0).abs() < 0.02, "x の広がり {}", span(0));
    assert!((span(1) - 12.0).abs() < 0.02, "y の広がり {}", span(1));
    let sim = simulate(kernel, &s, SimOptions::default()).unwrap();
    println!("{}", sim.summary());
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn a_morphed_spiral_covers_the_part_in_one_line_and_does_not_gouge() {
    let mut cam = reference_cam_3d();
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let base = OpBase::new(cam.setup, cam.tools, 4);
    insert_op(&mut cam, MORPH3D, "モーフィング", json(&Morph::new(base)));
    let s = recompute_ok(&mut cam, &reg, kernel);
    let path = s.paths.iter().find(|p| p.name == "モーフィング").unwrap();
    // 範囲は長方形 1 つなので、渦巻きは 1 本。早送りは入る所と抜ける所だけ
    let rapids = path
        .moves
        .iter()
        .filter(|m| matches!(m, jc_toolpath::Move::Rapid { .. }))
        .count();
    println!("早送り {rapids}");
    assert!(rapids <= 4, "早送り {rapids}");
    // 部品の外形(工具の半径だけ外まで)を回る
    let cut: Vec<[f64; 3]> = path
        .moves
        .iter()
        .filter(|m| matches!(m, jc_toolpath::Move::Linear { .. }))
        .map(|m| m.to())
        .collect();
    let span = |k: usize| {
        let v = cut.iter().map(|q| q[k]);
        v.clone().fold(f64::NEG_INFINITY, f64::max) - v.fold(f64::INFINITY, f64::min)
    };
    let (lo, hi) = (s.info.part_min, s.info.part_max);
    for k in 0..2 {
        let want = hi[k] - lo[k];
        assert!(
            span(k) > want && span(k) < want + 3.2,
            "{k}: {} / {want}",
            span(k)
        );
    }
    let sim = simulate(kernel, &s, SimOptions::default()).unwrap();
    println!("{}", sim.summary());
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn scallop_rings_cover_the_part_and_do_not_gouge() {
    let mut cam = reference_cam_3d();
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let base = OpBase::new(cam.setup, cam.tools, 4);
    insert_op(
        &mut cam,
        SCALLOP3D,
        "スキャロップ",
        json(&Scallop::new(base)),
    );
    let s = recompute_ok(&mut cam, &reg, kernel);
    let path = s.paths.iter().find(|p| p.name == "スキャロップ").unwrap();
    let rapids = path
        .moves
        .iter()
        .filter(|m| matches!(m, jc_toolpath::Move::Rapid { .. }))
        .count();
    println!("早送り {rapids}");
    // 輪から次の輪へは面をなぞって移る(輪ごとに上げない)。上げるのは、輪の分かれた先の
    // 別の所(四隅の穴・中央の穴・ポケット・ボスの上)へ移る時と、入る所・抜ける所だけ(16 回)
    assert!(rapids <= 20, "早送り {rapids}");
    let cut: Vec<[f64; 3]> = path
        .moves
        .iter()
        .filter(|m| matches!(m, jc_toolpath::Move::Linear { .. }))
        .map(|m| m.to())
        .collect();
    let span = |k: usize| {
        let v = cut.iter().map(|q| q[k]);
        v.clone().fold(f64::NEG_INFINITY, f64::max) - v.fold(f64::INFINITY, f64::min)
    };
    // 部品の外形(工具の半径 1.59 mm だけ外まで)を回る。いちばん外の輪は範囲の外の節点と
    // 内の節点の間を通るので、輪の格子(送り幅の 1/6)の半分まで外へ出る
    let (lo, hi) = (s.info.part_min, s.info.part_max);
    for k in 0..2 {
        let want = hi[k] - lo[k];
        assert!(
            span(k) > want && span(k) < want + 3.5,
            "{k}: {} / {want}",
            span(k)
        );
    }
    let sim = simulate(kernel, &s, SimOptions::default()).unwrap();
    println!("{}", sim.summary());
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn ramping_the_walls_goes_down_without_lifting_between_levels_and_does_not_gouge() {
    let mut cam = reference_cam_3d();
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let base = OpBase::new(cam.setup, cam.tools, 4);
    let ramp = Ramp3d {
        step: Some("1 mm".into()),
        depth: Some("11 mm".into()),
        ..Ramp3d::new(base.clone())
    };
    insert_op(&mut cam, RAMP3D, "ランプ", json(&ramp));
    // 比べる等高線(同じ工具・同じ段の幅・同じ深さ)
    let water = Waterline {
        base,
        leave: "0 mm".into(),
        step: Some("1 mm".into()),
        resolution: "0.1 mm".into(),
        depth: Some("11 mm".into()),
        ramp_angle: None,
        entry: None,
        order: Default::default(),
        limits: Default::default(),
    };
    insert_op(&mut cam, WATERLINE, "等高線", json(&water));
    let s = recompute_ok(&mut cam, &reg, kernel);
    let path = |name: &str| s.paths.iter().find(|p| p.name == name).unwrap();
    let rapids = |name: &str| {
        path(name)
            .moves
            .iter()
            .filter(|m| matches!(m, jc_toolpath::Move::Rapid { .. }))
            .count()
    };
    // 段ごとに上げ下げしないので、早送りは等高線より少ない
    let (r, w) = (rapids("ランプ"), rapids("等高線"));
    println!("早送り ランプ {r} / 等高線 {w}");
    assert!(r < w, "ランプ {r} / 等高線 {w}");
    // 平らに回るのは、いちばん下の輪(列ごとに 1 周)と開いた線だけ。下りながら回る所が大半
    let cut: Vec<[f64; 3]> = path("ランプ")
        .moves
        .iter()
        .filter(|m| matches!(m, jc_toolpath::Move::Linear { .. }))
        .map(|m| m.to())
        .collect();
    let (mut flat, mut down) = (0.0, 0.0);
    for w in cut.windows(2) {
        let run = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if (w[1][2] - w[0][2]).abs() < 1e-9 {
            flat += run;
        } else if w[1][2] < w[0][2] {
            down += run;
        }
    }
    println!("下りながら {down:.0} mm / 平ら {flat:.0} mm");
    assert!(down > flat, "下りながら {down} / 平ら {flat}");
    // 下りながら回る間に上がらない(穴・ボス・ポケットの縁で針のように跳ねない)。早送りの後の
    // 下降と、列から列へ移る所は除く
    let moves = &path("ランプ").moves;
    let mut rises = Vec::new();
    for w in moves.windows(2) {
        let both = w
            .iter()
            .all(|m| matches!(m, jc_toolpath::Move::Linear { .. }));
        let (a, b) = (w[0].to(), w[1].to());
        if both && b[2] > a[2] + 0.02 && b[2] < s.info.stock_max[2] - 1e-6 {
            rises.push((a, b));
        }
    }
    assert!(
        rises.is_empty(),
        "上がる所 {} 個: {:?}",
        rises.len(),
        &rises[..rises.len().min(3)]
    );
    let sim = simulate(kernel, &s, SimOptions::default()).unwrap();
    println!("{}", sim.summary());
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn adding_a_line_operation_skips_sketches_that_have_only_points() {
    // 3D の見本の最後のスケッチは穴の位置(点だけ)。選ばずに投影・彫刻を足すと、線のある
    // その前のスケッチを使う(点だけのスケッチを選ぶと、足した直後に誤りになる)
    for action in ["cam.add.project3d", "cam.add.engrave"] {
        let mut cam = reference_cam_3d();
        let setup = cam.setup;
        run_cam(&mut cam, action, setup, serde_json::json!({}));
        let m = modules();
        let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
        let added = cam.part.doc.data().features.last().unwrap();
        assert!(
            matches!(report.feature(added.id), Some(FeatureStatus::Ok { .. })),
            "{action}: {:?}",
            report.feature(added.id)
        );
    }
}

#[test]
fn roughing_is_simulated_on_a_coarser_grid_unless_the_operation_asks_for_fine() {
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let opt = SimOptions {
        cell: 0.1,
        rough_cell: 0.5,
        ..SimOptions::default()
    };
    let report = cam.part.doc.recompute(&reg, kernel);
    let all = setups(cam.part.doc.data(), &report, &reg);
    let s = &all[0];
    let cells = path_cells(s, &opt);
    let kinds: Vec<(&str, f64)> = s
        .sources
        .iter()
        .map(|p| p.type_id.as_str())
        .zip(cells.iter().copied())
        .collect();
    // 自動: 荒削りの種類(フェイス・ポケット・負荷一定・3D の荒削り)は粗い格子、ほかは細かい格子
    for (ty, c) in &kinds {
        let want = if [ADAPTIVE, POCKET, FACE, ROUGH3D].contains(ty) {
            0.5
        } else {
            0.1
        };
        assert_eq!(*c, want, "{kinds:?}");
    }
    assert!(kinds.iter().any(|k| k.1 == 0.5) && kinds.iter().any(|k| k.1 == 0.1));
    // 粗い格子と細かい格子をまたいで削っても、食い込みは出ない。結果は最後の加工の格子
    let t = std::time::Instant::now();
    let mixed = simulate(kernel, s, opt).unwrap();
    let mixed_time = t.elapsed();
    assert!(mixed.is_ok(), "{}", mixed.summary());
    assert_eq!(mixed.result.cell, *cells.last().unwrap());
    let t = std::time::Instant::now();
    let fine = simulate(
        kernel,
        s,
        SimOptions {
            rough_cell: 0.0,
            ..opt
        },
    )
    .unwrap();
    println!(
        "荒削り 0.5 mm: {mixed_time:?}、全部 0.1 mm: {:?}(削った体積 {:.0} / {:.0} mm³)",
        t.elapsed(),
        mixed.removed_volume,
        fine.removed_volume
    );
    assert!((mixed.removed_volume - fine.removed_volume).abs() < 0.05 * fine.removed_volume);
    // 加工の欄で「細かい」にすれば、荒削りでも細かい格子
    let rough = s.sources[cells.iter().position(|c| *c == 0.5).unwrap()].id;
    let mut e = cam.part.doc.data().feature(rough).unwrap().clone();
    // 経路は変わらないが、欄の値は機能のデータに残る
    e.data["sim_grid"] = serde_json::json!("fine");
    cam.part
        .doc
        .apply(jc_core::Transaction::new(
            "シミュレーションの精度",
            vec![jc_core::Op::SetFeatureData {
                id: rough,
                data: e.data,
            }],
        ))
        .unwrap();
    let report = cam.part.doc.recompute(&reg, kernel);
    let all = setups(cam.part.doc.data(), &report, &reg);
    let k = all[0].sources.iter().position(|p| p.id == rough).unwrap();
    assert_eq!(path_cells(&all[0], &opt)[k], 0.1);
}
