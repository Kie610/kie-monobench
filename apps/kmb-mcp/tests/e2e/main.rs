//! 通しの試験(e2e)。MCP の道具と同じ [`Session`] を、人や AI が使う順に動かし、
//! 見本ごとの結果(G-code・質量)を基準のファイル(`tests/golden/`)と比べる。
//!
//! 基準を作り直すとき(加工の結果を変えたとき)は、
//! `JC_BLESS=1 cargo nextest run -p kmb-mcp --test e2e` で書き直し、差分を読んでからコミットする。
//! 小数は 0.0015 までの違いを許す(PC とクラウドで C++ の丸めが違っても通るように)。

mod golden;

use kmb_mcp::Session;
use serde_json::{Value as Json, json};
use std::path::PathBuf;

fn temp_root(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("e2e-{name}"));
    std::fs::remove_dir_all(&d).ok();
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// エラーのない状態か(あれば理由つきで止める)。
fn assert_clean(st: &Json) {
    assert_eq!(st["errors"], 0, "{st:#}");
}

/// 質量の値を、基準のファイルに書く形の文字にする。
fn mass_text(m: &Json) -> String {
    let mut s = String::new();
    for (i, b) in m.as_array().expect("ボディの一覧").iter().enumerate() {
        let num = |v: &Json| format!("{:.3}", v.as_f64().expect("数"));
        let vec3 = |v: &Json| {
            v.as_array()
                .expect("3 つの数")
                .iter()
                .map(num)
                .collect::<Vec<_>>()
                .join(" ")
        };
        s += &format!("body {i}\n");
        s += &format!("volume {}\n", num(&b["volume"]));
        s += &format!("area {}\n", num(&b["area"]));
        s += &format!("min {}\n", vec3(&b["min"]));
        s += &format!("max {}\n", vec3(&b["max"]));
    }
    s
}

fn gcode(s: &mut Session) -> String {
    let r = s.gcode(None, None, None).expect("G-code を作れる");
    r["gcode"].as_str().expect("G-code の文字").to_string()
}

/// 保存して別の Session で開き直すと、同じ G-code になる。
fn reopened_gives_the_same_gcode(s: &mut Session, root: &std::path::Path, g: &str) {
    s.save(Some("part.json"), false).expect("保存できる");
    let mut t = Session::new(root.to_path_buf());
    assert_clean(&t.open("part.json").expect("開き直せる"));
    assert_eq!(gcode(&mut t), g, "開き直すと G-code が変わった");
}

#[test]
fn the_reference_part_has_the_expected_mass_and_survives_a_save() {
    let root = temp_root("part");
    let mut s = Session::new(root.clone());
    assert_clean(&s.sample(false, false).unwrap());
    let m = mass_text(&s.mass().unwrap());
    golden::check("part.mass.txt", &m);
    s.save(Some("part.json"), false).unwrap();
    let mut t = Session::new(root.clone());
    assert_clean(&t.open("part.json").unwrap());
    assert_eq!(mass_text(&t.mass().unwrap()), m);
}

#[test]
fn the_3d_cam_sample_writes_the_expected_gcode() {
    let root = temp_root("cam3d");
    let mut s = Session::new(root.clone());
    assert_clean(&s.sample(true, true).unwrap());
    let g = gcode(&mut s);
    golden::check("cam3d.nc", &g);
    reopened_gives_the_same_gcode(&mut s, &root, &g);
}

/// 同じ 2D の見本を 1 回だけ作って確かめる:
/// - 2D の加工の見本は基準どおりの G-code を書き、削っても食い込まず、開き直しても同じ
/// - 幅を変えると、形と経路が作り直される。取り消すと元の G-code に戻る
#[test]
fn the_2d_cam_sample_writes_the_expected_gcode_and_a_width_change_is_undone() {
    let root = temp_root("cam");
    let mut s = Session::new(root.clone());
    assert_clean(&s.sample(true, false).unwrap());
    let g = gcode(&mut s);
    golden::check("cam.nc", &g);
    let sim = s.simulate(None, None).unwrap();
    assert_eq!(sim["ok"], json!(true), "{sim:#}");
    reopened_gives_the_same_gcode(&mut s, &root, &g);

    let before = s.mass().unwrap()[0]["max"][0].as_f64().unwrap();
    assert_clean(&s.set_param("width", "75 mm").unwrap());
    let after = s.mass().unwrap()[0]["max"][0].as_f64().unwrap();
    assert!((after - before - 15.0).abs() < 1e-6, "{before} → {after}");
    golden::check("cam-width75.nc", &gcode(&mut s));
    let sim = s.simulate(None, None).unwrap();
    assert_eq!(sim["ok"], json!(true), "{sim:#}");
    assert_clean(&s.undo().unwrap());
    golden::check("cam.nc", &gcode(&mut s));
}
