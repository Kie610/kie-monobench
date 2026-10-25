//! NC プログラムのまとめ方(手動 NC・工具の順の書き出し・パターン)を基準部品で確かめる。

use crate::cam_tests::*;
use crate::reference_cam;
use jc_cam_commands::*;
use jc_core::FeatureStatus;
use jc_post_carvera_air::CarveraAir;

/// 拡張の操作を実行し、書き出したファイル(名前の後ろと中身と要約)を返す。
fn run_file(
    cam: &mut crate::ReferenceCam,
    id: &str,
    selected: Option<jc_core::FeatureId>,
    args: serde_json::Value,
) -> Result<(String, String, serde_json::Value), String> {
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = jc_core::Context {
        selected,
        args: &args,
        ..jc_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    match m.extensions.run(id, &cx)? {
        jc_core::Outcome::File {
            suffix,
            contents,
            info,
        } => Ok((suffix, String::from_utf8(contents).unwrap(), info)),
        other => panic!("{id} がファイルを書かない: {other:?}"),
    }
}

/// 手動 NC を加工を選んで足すと、その加工の後に入り、G-code では停止の前で主軸を止めて
/// 退避し、再開の後で主軸を回し直す(受け入れ (a) を見本の文書で)。
#[test]
fn a_manual_stop_added_after_an_operation_is_written_between_operations() {
    let mut cam = reference_cam();
    let top = cam.top_pocket;
    run_cam(&mut cam, ADD_MANUAL_NC, top, serde_json::Value::Null);
    let id = cam.part.doc.data().features.last().unwrap().id;
    let mut data = cam.part.doc.data().feature(id).unwrap().data.clone();
    assert_eq!(data["after"], serde_json::json!(cam.top_pocket.0));
    data["text"] = serde_json::json!("クランプを付け替える");
    set_data(&mut cam.part.doc, id, data);
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let s = setups(cam.part.doc.data(), &report, &m.registry).remove(0);
    assert_eq!(s.paths.len(), 6, "手動 NC は経路を持たない");
    assert_eq!(s.manual.len(), 1);
    assert_eq!(s.manual[0].before, 2, "ボスの周りの後、ポケットの前");
    let g = gcode(&s, &CarveraAir, &jc_post_carvera_air::profile()).unwrap();
    let lines: Vec<&str> = g.lines().collect();
    let at = lines.iter().position(|l| *l == "M600").expect(&g);
    let before = lines.iter().position(|l| *l == "(ボスの周り)").unwrap();
    let after = lines.iter().position(|l| *l == "(ポケット)").unwrap();
    assert!(before < at && at < after, "{g}");
    assert_eq!(lines[at - 4], "G0 Z20", "{g}");
    assert_eq!(lines[at - 3], "M5", "{g}");
    assert_eq!(lines[at - 2], "(停止: クランプを付け替える)", "{g}");
    // 再開の後: 次の加工(工具 T2 に替える)の前に主軸を回し直して待つ
    let spin = (at..lines.len())
        .find(|&i| lines[i].starts_with("M3 "))
        .unwrap();
    assert_eq!(lines[spin + 1], "G4 P2", "{g}");
    let first_cut = (at..lines.len())
        .find(|&i| lines[i].starts_with("G1"))
        .unwrap();
    assert!(spin < first_cut, "{g}");
    // G-code を読み直すシミュレーションも、停止の行で止まらない
    assert!(with_gcode_paths(&s, &g).is_ok());
    // 段取り表にも、加工の間に出る
    let sheet = setup_sheet(&s, &jc_post_carvera_air::profile());
    assert!(
        sheet.contains("手動 NC: 停止: クランプを付け替える"),
        "{sheet}"
    );
    // 停止は抑制すると入らない
    cam.part
        .doc
        .apply(jc_core::Transaction::new(
            "抑制",
            vec![jc_core::Op::SetSuppressed {
                id,
                suppressed: true,
            }],
        ))
        .unwrap();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let s = setups(cam.part.doc.data(), &report, &m.registry).remove(0);
    assert!(s.manual.is_empty());
}

/// 工具の順に並べ替えると工具交換が減る。残り削りがあれば並べ替えない(受け入れ (d))。
#[test]
fn exporting_by_tool_reduces_tool_changes_and_refuses_rest_machining() {
    let mut cam = reference_cam();
    let setup = cam.setup;
    // フェイスを T2 にすると、T2 → T1 → T2 の 3 回の交換になる
    let mut data = cam.part.doc.data().feature(cam.face).unwrap().data.clone();
    data["tool"] = serde_json::json!(2);
    set_data(&mut cam.part.doc, cam.face, data);
    let (_, g, info) = run_file(
        &mut cam,
        EXPORT_GCODE_BY_TOOL,
        Some(setup),
        serde_json::Value::Null,
    )
    .unwrap();
    assert_eq!(info["tool_changes_before"], 3);
    assert_eq!(info["tool_changes_after"], 2);
    assert_eq!(g.lines().filter(|l| l.starts_with("M6 ")).count(), 2, "{g}");
    assert!(g.find("(ボスの周り)").unwrap() < g.find("(上面のフェイス)").unwrap());
    // 残り削りを足すと並べ替えない
    let top = cam.top_pocket;
    run_cam(&mut cam, REST_POCKET, top, serde_json::Value::Null);
    let e = run_file(
        &mut cam,
        EXPORT_GCODE_BY_TOOL,
        Some(setup),
        serde_json::Value::Null,
    )
    .unwrap_err();
    assert!(e.contains("残り削り"), "{e}");
    // 前の工具を直径で打った残り削りも同じ
    let rest = cam
        .part
        .doc
        .data()
        .features
        .iter()
        .find(|f| f.data.get("rest_tool").is_some_and(|v| !v.is_null()))
        .unwrap()
        .clone();
    let mut data = rest.data;
    data["rest_tool"] = serde_json::Value::Null;
    data["rest_diameter"] = serde_json::json!("3.175 mm");
    set_data(&mut cam.part.doc, rest.id, data);
    let e = run_file(
        &mut cam,
        EXPORT_GCODE_BY_TOOL,
        Some(setup),
        serde_json::Value::Null,
    )
    .unwrap_err();
    assert!(e.contains("残り削り"), "{e}");
}

/// パターンを加工を選んで足すと、写しが同じセットアップの経路になる。全セットアップを 1 本にも書ける。
#[test]
fn a_pattern_of_an_operation_adds_copies_and_all_setups_export_as_one() {
    let mut cam = reference_cam();
    let setup = cam.setup;
    let hole = cam.center_hole;
    run_cam(&mut cam, ADD_PATTERN, hole, serde_json::Value::Null);
    let id = cam.part.doc.data().features.last().unwrap().id;
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
        panic!("{:?}", report.feature(id))
    };
    let s = setups(cam.part.doc.data(), &report, &m.registry).remove(0);
    assert_eq!(s.paths.len(), 7);
    let copy = s.paths.last().unwrap();
    let hole = s.paths.iter().find(|p| p.name == "中央の穴").unwrap();
    // 写しは X に間隔だけずれる
    let dx = copy.moves[0].to()[0] - hole.moves[0].to()[0];
    assert!(dx > 5.0, "{dx}");
    assert_eq!(copy.moves.len(), hole.moves.len());
    // 既定の間隔の写しはストックの中に収まるので、注意は出ない
    // (ストックの外を削る写しの注意は jc-cam-setup の pattern の試験で確かめる)
    let w = jc_core::warnings_of(outputs);
    assert!(w.is_empty(), "{w:?}");
    let (name, g, _) = run_file(&mut cam, EXPORT_GCODE_ALL, None, serde_json::Value::Null).unwrap();
    assert_eq!(name, "全セットアップ.nc");
    assert!(g.contains("(中央の穴 のパターン)"), "{g}");
    assert!(!g.contains("M600"), "セットアップが 1 つなら停止はない");
    let (name, html, _) = run_file(
        &mut cam,
        SETUP_SHEET_HTML,
        Some(setup),
        serde_json::Value::Null,
    )
    .unwrap();
    assert!(name.ends_with("加工指示書.html"));
    assert!(html.contains("<svg") && html.contains("mm/min"), "{html}");
    // 画面の点検で中身をブラウザで見るため(JC_SHEET_OUT=<ファイル> の時だけ書く)
    if let Ok(p) = std::env::var("JC_SHEET_OUT") {
        std::fs::write(p, &html).unwrap();
    }
}

/// ポケットをパターンで写すと、つなぎの記録も写しの動きに合う(下げたまま移る数が同じで、
/// 経路の決まりにも反しない)。
#[test]
fn a_pattern_keeps_the_link_records_of_its_copies() {
    let mut cam = reference_cam();
    let pocket = cam.pocket;
    run_cam(&mut cam, ADD_PATTERN, pocket, serde_json::Value::Null);
    let id = cam.part.doc.data().features.last().unwrap().id;
    // 写しを 2 つにする(記録の番号のずれは 2 つ目の写しで出る)
    let mut d = cam.part.doc.data().feature(id).unwrap().data.clone();
    d["count_x"] = serde_json::json!(3);
    set_data(&mut cam.part.doc, id, d);
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    assert!(
        matches!(report.feature(id), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        report.feature(id)
    );
    let s = setups(cam.part.doc.data(), &report, &m.registry).remove(0);
    // 写しと元の作ったままの経路を比べる(空振りを省くと、写しごとに省く所が違う)
    let copy = s.generated(s.paths.len() - 1);
    let src = s.generated(s.paths.iter().position(|p| p.name == "ポケット").unwrap());
    assert!(src.links.count(jc_toolpath::LinkKind::StayDown) > 0);
    assert!(
        jc_toolpath::check(copy).is_empty(),
        "{:?}",
        jc_toolpath::check(copy)
    );
    assert!(copy.moves.len() >= 2 * src.moves.len(), "写しが 2 つ");
    for k in [
        jc_toolpath::LinkKind::StayDown,
        jc_toolpath::LinkKind::Lift,
        jc_toolpath::LinkKind::Low,
        jc_toolpath::LinkKind::Full,
    ] {
        let per_copy = copy.moves.len() / src.moves.len();
        assert_eq!(copy.links.count(k), src.links.count(k) * per_copy, "{k:?}");
    }
    // 記録の範囲は写しの動きの中で、範囲の中は早送りか送りのつなぎ
    for l in &copy.links.links {
        assert!(l.start < l.end && l.end <= copy.moves.len(), "{l:?}");
    }
    // 空の動きは写しの数だけ(2 つ目からは写しの間の移りも入るので、少し多い)
    let motion = jc_toolpath::Motion::simple(3000.0);
    let per_copy = (copy.moves.len() / src.moves.len()) as f64;
    let (a, b) = (copy.air_minutes(&motion), src.air_minutes(&motion));
    assert!(
        a >= b * per_copy - 1e-9 && a < b * per_copy * 1.5 + 0.1,
        "{a} {b}"
    );
}
