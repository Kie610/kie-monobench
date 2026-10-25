//! 加工を機械に依存させない(セットアップごとの機械と、汎用のポスト)、汎用のレーザー加工、
//! プローブで高さを補正する(次に作る機能の加工の段、優先度 中)。

use crate::reference_cam;
use jc_cam_commands::*;
use jc_core::{FeatureId, FeatureStatus, Registry};
use jc_post::machines;
use jc_strategy_laser::{KerfSide, LASER, Laser, LaserMode, Lead};

fn registry() -> Registry {
    jc_bundle::registry()
}

thread_local! {
    // 形はカーネルごとに番号を振るので、1 つの試験の中では同じカーネルで計算し直す
    static KERNEL: std::sync::Arc<dyn jc_core::Kernel> = jc_bundle::kernel();
}

fn kernel() -> std::sync::Arc<dyn jc_core::Kernel> {
    KERNEL.with(|k| k.clone())
}

fn json<T: serde::Serialize>(v: &T) -> serde_json::Value {
    serde_json::to_value(v).expect("JSON にできる")
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
        .apply(jc_core::Transaction::new(
            name,
            vec![jc_core::Op::InsertFeature {
                index: None,
                entry: jc_core::FeatureEntry {
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
        .apply(jc_core::Transaction::new(
            "設定",
            vec![jc_core::Op::SetFeatureData { id, data }],
        ))
        .unwrap();
}

fn compute(cam: &mut crate::ReferenceCam) -> (jc_core::RecomputeReport, Vec<SetupPaths>) {
    let reg = registry();
    let report = cam.part.doc.recompute(&reg, kernel().as_ref());
    let s = setups(cam.part.doc.data(), &report, &reg);
    (report, s)
}

/// 組み立てと同じ選び方(セットアップの機械の値と、その値が選ぶ内蔵のポスト)で書く。
fn gcode_of(s: &SetupPaths) -> Result<String, String> {
    let cam = CamCommands::new(jc_bundle::machines(), jc_bundle::strategies());
    let m = cam.machine_for(&s.info);
    gcode(s, cam.post_for(&m)?.as_ref(), &m)
}

/// 書いた G-code を読み直すと、元と同じ数・同じ工具の経路に戻る(G-code で削って確かめられる)。
fn reads_back(s: &SetupPaths, g: &str) {
    let back = with_gcode_paths(s, g).unwrap_or_else(|e| panic!("{e}\n{g}"));
    let kinds = |p: &[jc_toolpath::Toolpath]| -> Vec<(u32, jc_cam_tool::ToolKind)> {
        p.iter().map(|t| (t.tool.number, t.tool.kind)).collect()
    };
    assert_eq!(kinds(&back.paths), kinds(&s.paths), "{g}");
}

fn warnings(report: &jc_core::RecomputeReport, id: FeatureId) -> Vec<String> {
    match report.feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => jc_core::warnings_of(outputs),
        other => panic!("{other:?}"),
    }
}

/// 既定の機械は Carvera Air のまま(古い文書もそう読む)。汎用の Fanuc 系に替えると、同じ経路を
/// O 番号・G55・工具長の補正つきで書き、Carvera Air では G55 は欄の段階で知らせて書かない。
#[test]
fn the_setup_machine_picks_the_post_and_its_way_of_writing() {
    let mut cam = reference_cam();
    let setup = cam.setup;
    assert!(
        cam.part
            .doc
            .data()
            .feature(setup)
            .unwrap()
            .data
            .get("machine")
            .is_none(),
        "既定の機械は保存に書かない"
    );
    let (_, s) = compute(&mut cam);
    assert_eq!(s[0].info.machine, jc_post_carvera_air::ID);
    let carvera = gcode_of(&s[0]).unwrap();
    assert!(carvera.contains("\nG54\n") && carvera.contains("\nM491\n"));

    set(&mut cam, setup, "wcs", serde_json::json!(2));
    set(&mut cam, setup, "program", serde_json::json!(1001));
    let (report, s) = compute(&mut cam);
    assert!(
        warnings(&report, setup)
            .iter()
            .any(|w| w.contains("Carvera Air では書き出せません")),
        "{:?}",
        warnings(&report, setup)
    );
    assert!(gcode_of(&s[0]).is_err());

    set(
        &mut cam,
        setup,
        "machine",
        serde_json::json!(machines::GENERIC_FANUC),
    );
    let (report, s) = compute(&mut cam);
    assert!(
        !warnings(&report, setup)
            .iter()
            .any(|w| w.contains("書き出せません")),
        "{:?}",
        warnings(&report, setup)
    );
    let g = gcode_of(&s[0]).unwrap();
    let lines: Vec<&str> = g.lines().collect();
    assert_eq!(&lines[..2], &["%", "O1001"], "{g}");
    assert!(g.contains("\nG55\n") && g.contains(" M6\nG43 H"), "{g}");
    assert!(!g.contains("M491") && g.ends_with("M30\n%\n"), "{g}");
    reads_back(&s[0], &g);
    // 同じ経路(機械は経路を変えない。1 段の深さの表を持つ負荷制御粗取りは見本に無い)
    let carvera_moves = carvera.lines().filter(|l| l.starts_with("G1")).count();
    let fanuc_moves = g.lines().filter(|l| l.starts_with("G1")).count();
    assert_eq!(carvera_moves, fanuc_moves);
}

fn laser_cam(mode: LaserMode) -> (crate::ReferenceCam, FeatureId) {
    let mut cam = reference_cam();
    // 工具ライブラリにレーザーを足す
    let tools = cam.tools;
    let mut lib: jc_cam_tool::ToolLibrary =
        serde_json::from_value(cam.part.doc.data().feature(tools).unwrap().data.clone()).unwrap();
    let n = lib.add_tools(vec![jc_cam_tool::ToolLibrary::laser_tool()], false)[0];
    let data = json(&lib);
    cam.part
        .doc
        .apply(jc_core::Transaction::new(
            "工具",
            vec![jc_core::Op::SetFeatureData { id: tools, data }],
        ))
        .unwrap();
    // 板の上面(高さ 10)に 10 mm の四角と、中に 4 mm の四角の穴
    let plate = cam.part.plate;
    let mut s = crate::sample::SketchBuilder::new(jc_sketch::SketchPlane::face(
        plate,
        format!("f{}:end", plate.0),
    ));
    let mut rect = |x0: f64, y0: f64, w: f64| {
        let p: Vec<_> = [(x0, y0), (x0 + w, y0), (x0 + w, y0 + w), (x0, y0 + w)]
            .iter()
            .map(|(x, y)| s.fixed_point(*x, *y))
            .collect();
        for k in 0..4 {
            s.line(p[k], p[(k + 1) % 4]);
        }
    };
    rect(20.0, 25.0, 10.0);
    rect(23.0, 28.0, 4.0);
    let sketch = insert_op(&mut cam, jc_sketch::SKETCH, "焼く形", json(&s.build()));
    let base = jc_cam_setup::OpBase::new(cam.setup, cam.tools, n);
    let id = insert_op(
        &mut cam,
        LASER,
        "焼き印",
        json(&Laser {
            mode,
            ..Laser::new(base, sketch)
        }),
    );
    (cam, id)
}

/// 塗りは四角の中を 0.1 mm おきの往復で埋め、穴は焼かない。Carvera Air では M321 で替え、焼く
/// 送りの行に S0.5、はみ出しに S0 を付け、M322 で戻す。焼くだけなので材料は削らない。
#[test]
fn a_laser_fill_burns_inside_the_shape_at_the_surface() {
    let (mut cam, id) = laser_cam(LaserMode::Fill);
    let (report, s) = compute(&mut cam);
    assert!(
        matches!(report.feature(id), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        report.feature(id)
    );
    let s = &s[0];
    let tp = s.paths.iter().find(|p| p.name == "焼き印").unwrap();
    assert_eq!(tp.laser_power(), Some(0.5));
    let burn: Vec<[f64; 3]> = tp
        .moves
        .iter()
        .filter(|m| matches!(m, jc_toolpath::Move::Linear { .. }))
        .map(|m| s.info.to_world(m.to()))
        .collect();
    // 焼くのは表面の高さだけ。上下に動かない
    assert!(
        burn.iter().all(|p| (p[2] - 10.0).abs() < 1e-6),
        "{:?}",
        &burn[..4]
    );
    // 行は 10 mm を 0.1 mm おき(約 100 行)
    let rows = burn
        .iter()
        .map(|p| (p[1] * 1000.0).round() as i64)
        .collect::<std::collections::BTreeSet<_>>();
    assert!((95..=101).contains(&rows.len()), "{}", rows.len());
    let g = gcode_of(s).unwrap();
    let at = g.find("(焼き印)").expect(&g);
    let tail = &g[at..];
    assert!(tail.contains("\nM321\n"), "{tail}");
    assert!(
        tail.contains(" S0.5\n") && tail.contains(" S0\n"),
        "{}",
        &tail[..600]
    );
    assert!(
        !tail[..tail.find("M322").unwrap()].contains("M3 S"),
        "主軸を回さない"
    );
    assert!(tail.contains("\nM322\n"), "{tail}");
    reads_back(s, &g);
    // 穴(23〜27)の中の行は、穴をはみ出しの S0 で越える
    let sim = simulate(kernel().as_ref(), s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
    // レーザーのない機械では書けない
    let mut fanuc = s.clone();
    fanuc.info.machine = machines::GENERIC_FANUC.into();
    fanuc.info.machine_values = machines::generic_fanuc();
    let e = gcode_of(&fanuc).unwrap_err();
    assert!(e.contains("レーザーがありません"), "{e}");
    // Grbl 系は M4 で点け(焦点の高さへ下りる間は S0)、焼く行に S500、M5 でやめる
    let mut grbl = s.clone();
    grbl.info.machine = machines::GENERIC_GRBL.into();
    grbl.info.machine_values = machines::generic_grbl();
    let g = gcode_of(&grbl).unwrap();
    let tail = &g[g.find("(焼き印)").unwrap()..];
    assert!(
        tail.contains("\nM4 S0\nG1 Z-6 F1200 S0\n"),
        "{}",
        &tail[..400]
    );
    assert!(
        tail.contains(" S500\n") && tail.contains("\nM5\n"),
        "{}",
        &tail[..400]
    );
    reads_back(&grbl, &g);
}

/// 線は四角の縁を 2 つともなぞる(閉じた線は回る)。工具がレーザーでなければ誤り。
#[test]
fn a_laser_line_traces_the_sketch_and_needs_a_laser_tool() {
    let (mut cam, id) = laser_cam(LaserMode::Line);
    let (report, s) = compute(&mut cam);
    assert!(matches!(report.feature(id), Some(FeatureStatus::Ok { .. })));
    let tp = s[0].paths.iter().find(|p| p.name == "焼き印").unwrap();
    let link = |i: usize| {
        tp.spans
            .iter()
            .any(|s| s.role == jc_toolpath::Role::Link && (s.start..s.end).contains(&i))
    };
    let burns = tp
        .moves
        .iter()
        .enumerate()
        .filter(|(i, m)| matches!(m, jc_toolpath::Move::Linear { .. }) && !link(*i))
        .count();
    assert_eq!(
        burns, 8,
        "四角 2 つの 4 辺ずつ(焦点の高さへ下りる出力 0 の送りは数えない)"
    );
    set(&mut cam, id, "tool", serde_json::json!(1));
    let (report, _) = compute(&mut cam);
    let Some(FeatureStatus::Error { message }) = report.feature(id) else {
        panic!("{:?}", report.feature(id))
    };
    assert!(message.contains("レーザーではありません"), "{message}");
}

/// 焼く動きのうち、役目が `role` の物(`None` なら役目の印の無い物)の点(作業座標)。
fn burn_points(s: &SetupPaths, name: &str, role: Option<jc_toolpath::Role>) -> Vec<[f64; 3]> {
    let tp = s.paths.iter().find(|p| p.name == name).unwrap();
    let role_of = |i: usize| {
        tp.spans
            .iter()
            .find(|s| (s.start..s.end).contains(&i))
            .map(|s| s.role)
    };
    tp.moves
        .iter()
        .enumerate()
        .filter(|(i, m)| matches!(m, jc_toolpath::Move::Linear { .. }) && role_of(*i) == role)
        .map(|(_, m)| s.info.to_world(m.to()))
        .collect()
}

/// 切り抜き: 部品を残す補正で、外周(20〜30)は切り口の幅の半分だけ外へ、穴(23〜27)は内へ
/// ずれる。進入(円弧)と退出(直線)は捨てる側(外周の外・穴の中)に付き、残す所には入らない。
#[test]
fn a_laser_cut_offsets_by_half_the_kerf_and_leads_in_from_the_scrap_side() {
    let (mut cam, id) = laser_cam(LaserMode::Line);
    for (k, v) in [
        ("kerf_side", json(&KerfSide::Outside)),
        ("kerf", serde_json::json!("0.2 mm")),
        ("lead_in", json(&Lead::Arc)),
        ("lead_out", json(&Lead::Line)),
        ("overcut", serde_json::json!("0.3 mm")),
    ] {
        set(&mut cam, id, k, v);
    }
    let (report, s) = compute(&mut cam);
    assert!(
        matches!(report.feature(id), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        report.feature(id)
    );
    let s = &s[0];
    let cut = burn_points(s, "焼き印", None);
    let xs = || cut.iter().map(|p| p[0]);
    let (lo, hi) = (xs().fold(f64::MAX, f64::min), xs().fold(f64::MIN, f64::max));
    assert!(
        (lo - 19.9).abs() < 1e-3 && (hi - 30.1).abs() < 1e-3,
        "{lo} {hi}"
    );
    let hole: Vec<f64> = cut
        .iter()
        .filter(|p| (22.0..28.0).contains(&p[0]) && (27.0..33.0).contains(&p[1]))
        .map(|p| p[0])
        .collect();
    let (hlo, hhi) = (
        hole.iter().copied().fold(f64::MAX, f64::min),
        hole.iter().copied().fold(f64::MIN, f64::max),
    );
    assert!(
        (hlo - 23.1).abs() < 1e-3 && (hhi - 26.9).abs() < 1e-3,
        "{hlo} {hhi}"
    );
    // 残す所(外周の内で穴の外)の真ん中には、進入も退出も入らない
    let leads = burn_points(s, "焼き印", Some(jc_toolpath::Role::Lead));
    assert!(!leads.is_empty());
    let kept = |p: &[f64; 3]| {
        let inside = |a: f64, b: f64| {
            p[0] > a + 1e-6 && p[0] < b - 1e-6 && p[1] > a + 5.0 + 1e-6 && p[1] < b + 5.0 - 1e-6
        };
        inside(19.9, 30.1) && !inside(23.1 - 2e-6, 26.9 + 2e-6)
    };
    assert!(leads.iter().all(|p| !kept(p)), "{leads:?}");
    let tp = s.paths.iter().find(|p| p.name == "焼き印").unwrap();
    let lead_spans = tp
        .spans
        .iter()
        .filter(|s| s.role == jc_toolpath::Role::Lead)
        .count();
    assert_eq!(lead_spans, 4, "形 2 つの進入と退出");
    let g = gcode_of(s).unwrap();
    reads_back(s, &g);
}

/// 貫通: 焼き始めで止まったまま焼く。Grbl 系は止まると消える M4 の代わりに M3 で G4 を挟み、
/// 動く間だけ焼く Carvera Air では書き出しを誤りにする。
#[test]
fn a_laser_pierce_dwells_with_the_beam_on() {
    let (mut cam, id) = laser_cam(LaserMode::Line);
    set(&mut cam, id, "pierce_time", serde_json::json!(0.5));
    let (_, s) = compute(&mut cam);
    let s = &s[0];
    let e = gcode_of(s).unwrap_err();
    assert!(e.contains("貫通の時間"), "{e}");
    let mut grbl = s.clone();
    grbl.info.machine = machines::GENERIC_GRBL.into();
    grbl.info.machine_values = machines::generic_grbl();
    let g = gcode_of(&grbl).unwrap();
    assert_eq!(g.matches("\nM3 S500\nG4 P0.5\nM4 S500\n").count(), 2, "{g}");
    reads_back(&grbl, &g);
}

/// ガルバノ(SCULPFUN G20): 塗りの行の外へはみ出さず、書き出しは LightBurn に取り込む SVG。
#[test]
fn a_galvo_laser_skips_the_overscan_and_writes_svg_for_lightburn() {
    let (mut cam, id) = laser_cam(LaserMode::Fill);
    let setup = cam.setup;
    set(
        &mut cam,
        setup,
        "machine",
        serde_json::json!(machines::SCULPFUN_G20),
    );
    let (report, s) = compute(&mut cam);
    assert!(
        matches!(report.feature(id), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        report.feature(id)
    );
    let s = &s[0];
    let tp = s.paths.iter().find(|p| p.name == "焼き印").unwrap();
    assert!(
        tp.moves
            .iter()
            .filter(|m| matches!(m, jc_toolpath::Move::Linear { .. }))
            .all(|m| {
                let x = s.info.to_world(m.to())[0];
                (20.0 - 1e-6..=30.0 + 1e-6).contains(&x)
            }),
        "はみ出しは 0"
    );
    // 見本の削る加工は、主軸の無いガルバノでは書けない
    let e = gcode_of(s).unwrap_err();
    assert!(e.contains("レーザーの加工だけ"), "{e}");
    let mut only = s.clone();
    only.paths.retain(|p| p.name == "焼き印");
    let svg = gcode_of(&only).unwrap();
    assert!(
        svg.starts_with("<?xml") && svg.contains("<svg "),
        "{}",
        &svg[..300]
    );
    assert!(svg.contains("<polyline"), "{}", &svg[..600]);
    assert_eq!(jc_post::file_extension(machines::LIGHTBURN_SVG), "svg");
}

/// 高さの補正: G-code の先頭で測るなら、削る範囲を 1 mm 狭めた格子の G32 を先頭に、M370 を
/// 終わりに出す。コントローラーで測るなら G-code には出さずコメントにし、段取り表に書く。
/// 補正の命令のない機械で G-code で測ると、知らせて書かない。
#[test]
fn leveling_probes_the_cut_area_once_at_the_start() {
    let mut cam = reference_cam();
    let setup = cam.setup;
    set(
        &mut cam,
        setup,
        "leveling",
        serde_json::json!({"mode": "gcode", "nx": 4, "ny": 3}),
    );
    let (_, s) = compute(&mut cam);
    let s = &s[0];
    let grid = s.level_grid().unwrap();
    let g = gcode_of(s).unwrap();
    let probe = g.lines().find(|l| l.starts_with("G32")).expect(&g);
    assert_eq!(
        probe,
        format!(
            "G32 R1 X{} Y{} A{} B{} I4 J3 H2",
            jc_post::num(grid.x),
            jc_post::num(grid.y),
            jc_post::num(grid.width),
            jc_post::num(grid.length)
        )
    );
    // 最初の工具交換の前に測り、最後に解除する
    assert!(g.find("G32").unwrap() < g.find("M6 T").unwrap());
    assert!(g.ends_with("M370\nM30\n"), "{}", &g[g.len() - 40..]);
    reads_back(s, &g);
    assert!(
        setup_sheet(s, &jc_post_carvera_air::profile())
            .contains("| 高さの補正 | G-code の先頭でプローブが")
    );

    set(
        &mut cam,
        setup,
        "leveling",
        serde_json::json!({"mode": "controller", "nx": 4, "ny": 3}),
    );
    let (_, s) = compute(&mut cam);
    let g = gcode_of(&s[0]).unwrap();
    assert!(!g.contains("G32") && !g.contains("M370"), "{g}");
    assert!(
        g.contains("(高さの補正: コントローラーで表面を 4×3 点測ってから始める"),
        "{g}"
    );

    set(
        &mut cam,
        setup,
        "leveling",
        serde_json::json!({"mode": "gcode"}),
    );
    set(
        &mut cam,
        setup,
        "machine",
        serde_json::json!(machines::GENERIC_FANUC),
    );
    let (report, s) = compute(&mut cam);
    assert!(
        warnings(&report, setup)
            .iter()
            .any(|w| w.contains("補正する命令を持ちません"))
    );
    assert!(gcode_of(&s[0]).is_err());
}

/// 知らない機械 ID を、セットアップの機械の値を引く関数が黙って Carvera Air の値にしない
/// (1 段の深さ・レーザーの加速度を戦略がここから読む)。文書の道では、セットアップの評価が
/// 先に「マシンライブラリにありません」で止めるので経路は作られない。値を引く関数も黙って
/// 替えず、誤りを返す。
#[test]
fn an_unknown_machine_is_an_error_not_carvera_air() {
    let mut cam = reference_cam();
    let (_, s) = compute(&mut cam);
    assert_eq!(s[0].info.machine_profile().id, jc_post_carvera_air::ID);
    let mut data = jc_cam_setup::Setup::new(FeatureId(1));
    data.machine = "no-such-machine".into();
    let e = data
        .machine_values(&jc_bundle::machines())
        .expect_err("知らない機械");
    assert!(e.contains("no-such-machine"), "{e}");
    let setup = cam.setup;
    set(
        &mut cam,
        setup,
        "machine",
        serde_json::json!("no-such-machine"),
    );
    let (report, s) = compute(&mut cam);
    match report.feature(setup) {
        Some(FeatureStatus::Error { message }) => {
            assert!(message.contains("no-such-machine"), "{message}")
        }
        other => panic!("{other:?}"),
    }
    assert!(s.is_empty() || s.iter().all(|s| s.paths.is_empty()));
}

/// セットアップは機械の値を文書に写して持ち、経路・検査・ポストはその値を使う(同じ G-code に
/// ライブラリの値と設定で直した値の 2 通りが混ざらない)。設定を後で直しても、今あるセットアップの
/// 結果は変わらない。
#[test]
fn the_setup_keeps_its_own_copy_of_the_machine_values() {
    let mut cam = reference_cam();
    let setup = cam.setup;
    let mut mine = jc_post_carvera_air::profile();
    mine.name = "手元の Carvera Air".into();
    mine.travel[0] = 280.0;
    set(
        &mut cam,
        setup,
        "machine_values",
        serde_json::to_value(&mine).unwrap(),
    );
    set(&mut cam, setup, "wcs", serde_json::json!(2));
    let (report, s) = compute(&mut cam);
    assert_eq!(s[0].info.machine_profile(), &mine);
    // 欄の段階の知らせも、写した値(名前)で出る
    assert!(
        warnings(&report, setup)
            .iter()
            .any(|w| w.contains("手元の Carvera Air では書き出せません")),
        "{:?}",
        warnings(&report, setup)
    );
    // 書き出しの機械の値はセットアップの写し(設定の値を持つ CamCommands を作り直しても同じ)
    let cam_cmds = CamCommands::new(jc_bundle::machines(), jc_bundle::strategies());
    assert_eq!(cam_cmds.machine_for(&s[0].info), mine);
    // 欄で機械を替えると、写しは使わずその機械の初めの値になる
    set(
        &mut cam,
        setup,
        "machine",
        serde_json::json!(machines::GENERIC_FANUC),
    );
    let (_, s) = compute(&mut cam);
    assert_eq!(s[0].info.machine_profile(), &machines::generic_fanuc());
}

/// Carvera Air の機械の形で削ってみる。見本はそのまま通り、ストックを
/// ベッドの右へずらすと作業範囲の X をはみ出し、背の高いクランプは主軸の上の Z の台に当たる。
/// 形の無い機械(汎用の Fanuc 系)は機械との当たりを見ない。
#[test]
fn the_machine_shape_finds_travel_and_head_hits() {
    let mut cam = reference_cam();
    let setup = cam.setup;
    let k = kernel();
    let (_, s) = compute(&mut cam);
    let shape = machine_shape(&s[0].info).expect("Carvera Air は機械の形を持つ");
    // ストックの手前左の角は L 字の角(ベッドの角から 15, 15)。作業範囲はベッドの角から 3, 11
    let i = &s[0].info;
    assert_eq!(
        shape.travel.0,
        [i.stock_min[0] - 12.0, i.stock_min[1] - 4.0],
        "{shape:?}"
    );
    let r = simulate(k.as_ref(), &s[0], SimOptions::default()).unwrap();
    assert!(r.machine_hits.is_empty(), "{}", r.summary());

    // ベッドの上の置き場所 X 280 mm: 作業範囲(L 字の角から 288 mm まで)の外まで削りに行く
    set(&mut cam, setup, "bed_x", serde_json::json!("280 mm"));
    let (_, s) = compute(&mut cam);
    let r = simulate(k.as_ref(), &s[0], SimOptions::default()).unwrap();
    let hit = r
        .machine_hits
        .iter()
        .find(|h| {
            h.kind
                == MachineKind::Travel {
                    axis: 0,
                    high: true,
                }
        })
        .unwrap_or_else(|| panic!("{}", r.summary()));
    assert!(hit.event.amount > 0.0);
    assert!(!r.is_ok() && r.summary().contains("X の作業範囲を+側へはみ出す"));

    // ストックの脇 10 mm の所に高さ 80 mm のクランプ: 刃とホルダーは届かないが、径 52 mm の主軸
    // (主軸の鼻はホルダーの上)が当たる
    set(&mut cam, setup, "bed_x", serde_json::Value::Null);
    let (_, s) = compute(&mut cam);
    let i = &s[0].info;
    let size = [
        i.stock_max[0] - i.stock_min[0],
        i.stock_max[1] - i.stock_min[1],
    ];
    set(
        &mut cam,
        setup,
        "fixtures",
        serde_json::json!([{"at": [size[0] + 15.0, 0.0], "size": [10.0, 10.0, 80.0]}]),
    );
    let (_, s) = compute(&mut cam);
    assert_eq!(s[0].info.fixtures.len(), 1, "{:?}", s[0].info.fixtures);
    let r = simulate(k.as_ref(), &s[0], SimOptions::default()).unwrap();
    assert!(
        r.machine_hits
            .iter()
            .any(|h| h.kind == MachineKind::HeadFixture(HeadPart::Spindle)),
        "{}",
        r.summary()
    );
    assert!(r.fixture_hits.is_empty(), "{}", r.summary());

    // 形の無い機械は見ない
    set(&mut cam, setup, "fixtures", serde_json::json!([]));
    set(
        &mut cam,
        setup,
        "machine",
        serde_json::json!(machines::GENERIC_FANUC),
    );
    let (_, s) = compute(&mut cam);
    assert!(machine_shape(&s[0].info).is_none());
    let r = simulate(k.as_ref(), &s[0], SimOptions::default()).unwrap();
    assert!(r.machine_hits.is_empty() && r.bed_depth == 0.0);
}
