//! 手を加えない基準の加工(2D)を 1 回だけ計算し、それを読むだけの確かめを続けて流す。
//! 確かめごとに見本を作り直さず、シミュレーションも 1 回で済ませる。
//! 確かめは 1 つずつ独立していて、1 つが失敗しても残りを流し、最後に失敗した名前を並べる。
//! 文書や経路を変える試験は、計算し直しが要るので `cam_tests` に置く。

use crate::{ReferenceCam, reference_cam};
use kmb_cam_commands::*;
use kmb_core::{FeatureStatus, RecomputeReport};
use kmb_post_carvera_air::CarveraAir;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// 計算済みの基準の加工。
struct Scene {
    m: kmb_core::Modules,
    cam: ReferenceCam,
    report: RecomputeReport,
    setup: SetupPaths,
    /// 全部の経路のシミュレーション(既定の設定)
    sim: SimReport,
}

impl Scene {
    fn new() -> Self {
        let m = kmb_bundle::modules();
        let mut cam = reference_cam();
        let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
        for f in &report.features {
            assert!(
                matches!(f.status, FeatureStatus::Ok { .. }),
                "{} {}: {:?}",
                f.id,
                f.name,
                f.status
            );
        }
        let s = setups(cam.part.doc.data(), &report, &m.registry);
        assert_eq!(s.len(), 1);
        let setup = s.into_iter().next().unwrap();
        let sim = simulate(m.kernel.as_ref(), &setup, SimOptions::default()).unwrap();
        println!("{}", sim.summary());
        Self {
            m,
            cam,
            report,
            setup,
            sim,
        }
    }

    fn kernel(&self) -> &dyn kmb_core::Kernel {
        self.m.kernel.as_ref()
    }

    fn context<'a>(
        &'a self,
        selected: Option<kmb_core::FeatureId>,
        args: &'a serde_json::Value,
    ) -> kmb_core::Context<'a> {
        kmb_core::Context {
            selected,
            args,
            ..kmb_core::Context::new(
                self.cam.part.doc.data(),
                &self.report,
                &self.m.registry,
                &self.m.kernel,
            )
        }
    }
}

/// 確かめの名前と中身。
type Check = (&'static str, fn(&Scene));

#[test]
fn the_reference_cam_as_built() {
    let scene = Scene::new();
    let checks: &[Check] = &[
        ("2 本のエンドミルで削る", machined_with_two_endmills),
        ("段取り表", setup_sheet_lists_stock_origin_and_tools),
        ("書いた G-code で同じ形", written_gcode_cuts_the_same_shape),
        (
            "ボスの周りに柱が残らない",
            pocket_around_the_boss_leaves_no_pillar,
        ),
        ("入る位置を塗る所", entry_step_paints_where_the_helix_fits),
        (
            "選んだ加工だけを書き出す",
            only_the_selected_operation_is_exported,
        ),
    ];
    let failed: Vec<&str> = checks
        .iter()
        .filter(|(name, check)| {
            println!("--- {name}");
            catch_unwind(AssertUnwindSafe(|| check(&scene))).is_err()
        })
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "失敗した確かめ: {failed:?}");
}

fn machined_with_two_endmills(sc: &Scene) {
    let s = &sc.setup;
    assert_eq!(s.paths.len(), 6);
    let program = s.program();
    let tools: Vec<u32> = program.tools().iter().map(|t| t.number).collect();
    assert_eq!(tools, vec![1, 2]);
    let g = gcode(s, &CarveraAir, &kmb_post_carvera_air::profile()).unwrap();
    assert!(g.contains("M6 T1") && g.contains("M6 T2"));
    assert!(g.trim_end().ends_with("M30"));
    // 時間の目安は加減速と角の速さを見込むので、長さを速さで割るだけより長い
    let machine = kmb_post_carvera_air::profile();
    let simple = program.minutes(&kmb_toolpath::Motion::simple(machine.rapid_feed));
    let planned = program.minutes(&machine.motion());
    println!("時間の目安: 加減速込み {planned:.2} 分、割るだけ {simple:.2} 分");
    assert!(planned > simple && planned < simple * 3.0);
    assert!(g.contains(&format!("(時間の目安 {planned:.1} 分")));

    let sim = &sc.sim;
    for (name, v) in &sim.removed_by_op {
        println!("  {name}: {v:.1} mm³");
    }
    assert!(sim.is_ok(), "{}", sim.summary());
    // 残るのはポケットの内側の角(工具の半径の丸み)だけ
    let (volume, _, _) = crate::reference_values(60.0);
    let stock = 64.0 * 44.0 * 16.0;
    assert!(
        (sim.stock_volume - stock).abs() < 1.0,
        "{}",
        sim.stock_volume
    );
    let corners = 4.0 * 1.5875f64.powi(2) * (1.0 - std::f64::consts::PI / 4.0) * 4.0;
    assert!(
        sim.remaining_volume < corners * 1.5 + 5.0,
        "削り残し {} mm³(角の分の目安 {corners:.1})",
        sim.remaining_volume
    );
    let left = sim.stock_volume - sim.removed_volume;
    assert!(
        (left - volume).abs() < sim.remaining_volume + 0.01 * volume,
        "残った体積 {left} と部品 {volume}"
    );
}

fn setup_sheet_lists_stock_origin_and_tools(sc: &Scene) {
    let sheet = setup_sheet(&sc.setup, &kmb_post_carvera_air::profile());
    assert!(sheet.contains("# 段取り表: セットアップ 1"), "{sheet}");
    assert!(sheet.contains("| 作業座標系 | G54 |"), "{sheet}");
    assert!(sheet.contains("合計の時間の目安"), "{sheet}");
    // 工具は 1 本 1 行で、使う順(6 mm が先)
    let t1 = sheet.find("| T1 |").expect("T1");
    let t2 = sheet.find("| T2 |").expect("T2");
    assert!(t1 < t2);
    assert_eq!(sheet.lines().filter(|l| l.starts_with("| T1 |")).count(), 1);
}

fn written_gcode_cuts_the_same_shape(sc: &Scene) {
    let s = &sc.setup;
    let kernel = sc.kernel();
    let machine = kmb_post_carvera_air::profile();
    let g = gcode(s, &CarveraAir, &machine).unwrap();
    let from_g = with_gcode_paths(s, &g).unwrap();
    // 経路の名前はコメントから戻る
    let names = |s: &SetupPaths| s.paths.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&from_g), names(s));
    let opt = SimOptions::default();
    let a = &sc.sim;
    let b = simulate(kernel, &from_g, opt).unwrap();
    assert!(b.is_ok(), "{}", b.summary());
    // 数を 3 桁に丸めた分だけ、縁のセルが入れ替わることはある
    let differ = |x: &ZMap, y: &ZMap| {
        x.z.iter()
            .zip(&y.z)
            .filter(|(p, q)| (*p - *q).abs() > 0.05)
            .count() as f64
            / x.z.len() as f64
    };
    assert!(
        differ(&a.result, &b.result) < 0.002,
        "{}",
        differ(&a.result, &b.result)
    );
    assert!(
        (a.removed_volume - b.removed_volume).abs() < 0.002 * a.removed_volume,
        "{} {}",
        a.removed_volume,
        b.removed_volume
    );
    // G-code の誤り(ポケットの深さを 1 mm 深く書いた)は、食い込みとして出る
    let i = g.find("(ポケット)").unwrap();
    // 次の操作のコメントまで
    let end = g[i + 1..].find("\n(").map_or(g.len(), |e| i + 2 + e);
    let mut broken = g[..i].to_string();
    for l in g[i..end].lines() {
        match l.split_whitespace().find_map(|w| w.strip_prefix('Z')) {
            Some(z) if l.starts_with("G1") || !l.starts_with("G0") => {
                let deeper: f64 = z.parse::<f64>().unwrap() - 1.0;
                broken.push_str(&l.replace(&format!("Z{z}"), &format!("Z{deeper}")));
            }
            _ => broken.push_str(l),
        }
        broken.push('\n');
    }
    broken.push_str(&g[end..]);
    let c = simulate(kernel, &with_gcode_paths(s, &broken).unwrap(), opt).unwrap();
    assert!(!c.gouges.is_empty(), "{}", c.summary());
}

fn pocket_around_the_boss_leaves_no_pillar(sc: &Scene) {
    // 送り幅(6 mm の 0.6 倍 = 3.6 mm)が半径より大きいと、ボスと外周の間の細い所で次の輪が
    // 消え、その真ん中に高さ 5 mm の柱が残っていた(2026-10-14 までの版、(49, 37) の辺り)
    let mut s = sc.setup.clone();
    assert_eq!(s.paths[1].name, "ボスの周り");
    s.paths.truncate(2);
    let sim = simulate(sc.kernel(), &s, SimOptions::default()).unwrap();
    // ボスの中心(49, 22)、半径 5。壁の際は網の折れ線の分だけ残りに数えるので外す
    let pillars: Vec<_> = sim
        .leftovers
        .iter()
        .filter(|(p, d)| {
            let r = (p[0] - 49.0).hypot(p[1] - 22.0);
            // 差が 9 mm を超えるのは、まだ削らない穴と外形の外
            *d > 1.0 && *d < 9.0 && r > 5.3 && r < 16.0
        })
        .collect();
    assert!(pillars.is_empty(), "{pillars:?}");
}

fn entry_step_paints_where_the_helix_fits(sc: &Scene) {
    let args = serde_json::Value::Null;
    let cx = sc.context(Some(sc.cam.pocket), &args);
    let h =
        sc.m.extensions
            .handles(PLACE_ENTRY, &cx)
            .expect("入る位置の手順");
    assert!(h[0].label.contains("らせん"), "{}", h[0].label);
    // ポケット(左下 (8, 8) から 15 × 12)を、工具の半径とらせんの半径だけ内側へずらした所
    let s = &sc.setup;
    let tp = s.paths.iter().find(|p| p.name == "ポケット").unwrap();
    let (r, rh) = (tp.tool.radius(), 0.5 * tp.tool.radius());
    assert!(!h[0].outline.is_empty());
    let pts: Vec<[f64; 3]> = h[0].outline.iter().flatten().copied().collect();
    let (lo, hi) = pts.iter().fold(
        ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]),
        |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        },
    );
    let inset = r + rh;
    assert!(
        (lo[0] - (8.0 + inset)).abs() < 0.3 && (hi[0] - (23.0 - inset)).abs() < 0.3,
        "{lo:?} {hi:?} {inset}"
    );
    assert!(
        (lo[1] - (8.0 + inset)).abs() < 0.3 && (hi[1] - (20.0 - inset)).abs() < 0.3,
        "{lo:?} {hi:?} {inset}"
    );
    // 塗る所はストックの上面の高さ
    let top = s.info.to_world([0.0, 0.0, s.info.stock_max[2]])[2];
    assert!(pts.iter().all(|p| (p[2] - top).abs() < 1e-6));
}

/// 選んだ加工だけを書き出すと、その加工のコメントと動きだけが入る(受け入れ (b))。
fn only_the_selected_operation_is_exported(sc: &Scene) {
    let run = |selected, args: serde_json::Value| match sc
        .m
        .extensions
        .run(EXPORT_GCODE_SELECTED, &sc.context(selected, &args))?
    {
        kmb_core::Outcome::File {
            suffix,
            contents,
            info,
        } => Ok::<_, String>((suffix, String::from_utf8(contents).unwrap(), info)),
        other => panic!("ファイルを書かない: {other:?}"),
    };
    let (contour, setup, face) = (sc.cam.contour, sc.cam.setup, sc.cam.face);
    let (name, g, info) = run(Some(contour), serde_json::Value::Null).unwrap();
    assert_eq!(name, "セットアップ1-外形.nc");
    assert!(g.contains("(外形)") && g.contains("M6 T2"), "{g}");
    for other in [
        "(上面のフェイス)",
        "(ボスの周り)",
        "(ポケット)",
        "(中央の穴)",
        "M6 T1",
    ] {
        assert!(!g.contains(other), "{other}: {g}");
    }
    assert_eq!(info["operations"], serde_json::json!([contour.0]));
    // 指定 operations で複数
    let (_, g, _) = run(None, serde_json::json!({"operations": [face.0, contour.0]})).unwrap();
    assert!(g.contains("(上面のフェイス)") && g.contains("(外形)") && !g.contains("(ポケット)"));
    // セットアップを選んで使うと誤り
    assert!(run(Some(setup), serde_json::Value::Null).is_err());
}
