//! CLI(`kmb`)を実際に起動して確かめる試験。画面の無い同じ道(`kmb_session::Headless`)と結果を比べ、
//! CLI だけが別の作り方をして結果が変わる所を捕まえる。

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 試験ごとの作業フォルダ。利用者の設定を読まないよう、データフォルダもここに向ける。
fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kmb-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `kmb` を引数 `args` で走らせ、(成功したか, 標準出力, 標準エラー)を返す。
fn jc(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_kmb"))
        .args(args)
        .current_dir(dir)
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("APPDATA", dir.join("data"))
        .output()
        .expect("kmb を起動できる");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn ok(dir: &Path, args: &[&str]) -> String {
    let (ok, out, err) = jc(dir, args);
    assert!(ok, "kmb {args:?} が失敗した: {out}{err}");
    out
}

/// `kmb` を走らせ、終了の番号を返す。
fn exit_code(dir: &Path, args: &[&str]) -> i32 {
    Command::new(env!("CARGO_BIN_EXE_kmb"))
        .args(args)
        .current_dir(dir)
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("APPDATA", dir.join("data"))
        .output()
        .expect("kmb を起動できる")
        .status
        .code()
        .expect("終了の番号")
}

/// 終了の番号は誤りの種類で分ける(壊れた入力、食い込みを見つけたシミュレーション、ファイルが
/// 無いのを、呼ぶ側が見分けられる)。
#[test]
fn the_exit_code_tells_the_kind_of_problem() {
    let d = temp_dir("exit-code");
    ok(&d, &["sample", "a.json"]);
    // 1 入力の誤り(引数の打ち間違い)
    assert_eq!(
        exit_code(
            &d,
            &[
                "run",
                "a.json",
                "io.export_stl",
                "--arg",
                "tolerence=0.1",
                "--out",
                "x.stl"
            ]
        ),
        1
    );
    // 引数そのものの誤り(知らないコマンド・引数なし・要る引数が無い)も入力の誤り。clap の 2 にしない
    assert_eq!(exit_code(&d, &["no-such-command"]), 1);
    assert_eq!(exit_code(&d, &[]), 1);
    assert_eq!(exit_code(&d, &["run"]), 1);
    // 使い方の表示は誤りではない
    assert_eq!(exit_code(&d, &["--help"]), 0);
    // 4 今は使えない操作(スケッチを選んでいない)
    assert_eq!(
        exit_code(
            &d,
            &["run", "a.json", "cad.sketch.export_dxf", "--out", "x.dxf"]
        ),
        4
    );
    // 5 ファイルを読めない・書けない
    assert_eq!(exit_code(&d, &["actions", "無い.json"]), 5);
    assert_eq!(
        exit_code(
            &d,
            &[
                "run",
                "a.json",
                "io.export_stl",
                "--out",
                "無いフォルダ/x.stl"
            ]
        ),
        5
    );
    assert_eq!(exit_code(&d, &["sample", "a.json"]), 5);
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// パラメータの名前を替えると、その名前を使う式も書き換える(MCP と同じ)。書き換えないと、
/// 次に開いた時に式が「パラメータが無い」で止まる。
#[test]
fn renaming_a_parameter_rewrites_the_expressions_that_use_it() {
    let d = temp_dir("rename");
    ok(&d, &["new", "a.json"]);
    ok(&d, &["param", "add", "a.json", "w", "10 mm"]);
    ok(&d, &["param", "add", "a.json", "h", "w * 2"]);
    ok(&d, &["param", "rename", "a.json", "w", "width"]);
    let doc = read(&d.join("a.json"));
    let expr = |name: &str| {
        doc["params"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .map(|p| p["expr"].as_str().unwrap().to_string())
    };
    assert_eq!(expr("width").as_deref(), Some("10 mm"), "{doc}");
    assert_eq!(expr("h").as_deref(), Some("width * 2"), "{doc}");
    let w = kmb_session::Headless::open(kmb_bundle::modules(), &d.join("a.json")).unwrap();
    assert_eq!(w.report().error_count(), 0, "{}", w.report_text());
    std::fs::remove_dir_all(&d).ok();
}

/// `--cell` を省けば操作の既定(と設定の格子・荒削りの格子)に任せ、同じセットアップで画面・CLI・
/// MCP の結果がそろう。
#[test]
fn simulating_without_a_cell_uses_the_same_grid_as_the_screen() {
    let d = temp_dir("sim");
    ok(&d, &["sample", "a.json", "--cam"]);
    let cli = ok(&d, &["sim", "a.json"]);
    let modules = kmb_bundle::modules();
    let mut w = kmb_session::Headless::open(modules, &d.join("a.json")).unwrap();
    let id = w
        .action_for_role(&kmb_interchange::role::action::SIMULATE)
        .unwrap();
    let setup = w
        .doc()
        .data()
        .features
        .iter()
        .find(|f| f.type_id == "cam.setup@1")
        .map(|f| f.id);
    let kmb_session::Ran::Report(r) = w.run(&id, setup, None, &Value::Null).unwrap() else {
        panic!("シミュレーションの結果は報告");
    };
    let screen = r.info["summary"].as_str().unwrap_or(&r.text).to_string();
    assert_eq!(cli.lines().next(), screen.lines().next(), "{cli}");
    std::fs::remove_dir_all(&d).ok();
}

/// `.dxf`・`.svg` は、スケッチの書き出し(スケッチを選んでいないと使えない)でなく、ボディの書き出しの
/// 役割の操作から選ぶ。
#[test]
fn views_are_exported_to_dxf_and_svg_from_the_command_line() {
    let d = temp_dir("export");
    ok(&d, &["sample", "a.json"]);
    for ext in ["dxf", "svg", "stl", "step", "3mf"] {
        let out = format!("a.{ext}");
        // 視点は、受け取る書き出し(今の視点の DXF・SVG)にだけ渡す。受け取らない操作に渡すと誤り
        let view: &[&str] = if matches!(ext, "dxf" | "svg") {
            &["--view", "top"]
        } else {
            &[]
        };
        ok(&d, &[&["export", "a.json", &out][..], view].concat());
        let bytes = std::fs::read(d.join(&out)).unwrap();
        assert!(bytes.len() > 100, "{out} が空: {} バイト", bytes.len());
    }
    let dxf = std::fs::read_to_string(d.join("a.dxf")).unwrap();
    assert!(dxf.contains("ENTITIES"), "{dxf}");
    let svg = std::fs::read_to_string(d.join("a.svg")).unwrap();
    assert!(svg.contains("<svg"), "{svg}");
    // ボディの書き出しでない操作(部品表の .csv)は export では選ばない
    let (done, _, err) = jc(&d, &["export", "a.json", "a.csv"]);
    assert!(!done, "部品表は export の書き出しでない");
    assert!(err.contains(".stl"), "{err}");
    // 拡張子だけでは選ばれない書き出しは --action で(隠れ線つきの DXF)
    let hidden = ok(
        &d,
        &[
            "export",
            "a.json",
            "h.dxf",
            "--view",
            "iso",
            "--action",
            "io.export_view_dxf_hidden",
        ],
    );
    let _ = hidden;
    assert!(
        std::fs::read_to_string(d.join("h.dxf"))
            .unwrap()
            .contains("HIDDEN")
    );
    let (done, _, err) = jc(&d, &["export", "a.json", "a.csv"]);
    assert!(!done, "部品表は export の書き出しでない");
    assert!(err.contains(".stl"), "{err}");
    std::fs::remove_dir_all(&d).ok();
}

/// DXF の図形(`  0` で始まる組)を並べ替えた一覧。SMLib は同じ形でも面と辺の並びが毎回違うので、
/// 線の順は比べられない(線の組は同じ)。
fn dxf_entities(dxf: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(dxf);
    let mut v: Vec<String> = text.split("\n  0\n").map(str::to_string).collect();
    v.sort();
    v
}

/// 書き出しの CLI は、利用者が書かない限り許容差を渡さない(操作の既定に任せる)。
#[test]
fn exporting_without_a_tolerance_matches_the_action_default() {
    let d = temp_dir("tolerance");
    ok(&d, &["sample", "a.json"]);
    ok(&d, &["export", "a.json", "cli.dxf", "--view", "iso"]);
    ok(
        &d,
        &[
            "export",
            "a.json",
            "fine.dxf",
            "--view",
            "iso",
            "--tolerance",
            "0.01",
        ],
    );
    let modules = kmb_bundle::modules();
    let mut w = kmb_session::Headless::open(modules, &d.join("a.json")).unwrap();
    let id = w.save_action_for(&d.join("x.dxf"), None).unwrap();
    let kmb_session::Ran::File { contents, .. } = w
        .run(&id, None, None, &serde_json::json!({"view": "iso"}))
        .unwrap()
    else {
        panic!("DXF は 1 つのファイル");
    };
    let cli = dxf_entities(&std::fs::read(d.join("cli.dxf")).unwrap());
    assert_eq!(cli, dxf_entities(&contents));
    // 比べ方が許容差の違いを見分けられること(斜めから見ると、穴の輪郭の線が網の細かさで変わる)
    assert_ne!(
        cli,
        dxf_entities(&std::fs::read(d.join("fine.dxf")).unwrap())
    );
    std::fs::remove_dir_all(&d).ok();
}

/// CLI は文書の変更を自分で組み立てない(画面・MCP と同じ道 `Headless`・`kmb_session::edit` を通す)。
/// CLI は核の変更を直に作らない(名前の変更なら、直に作ると式の書き換えを飛ばす)。
#[test]
fn the_cli_does_not_build_core_operations_itself() {
    let src = include_str!("../src/main.rs");
    for (n, line) in src.lines().enumerate() {
        assert!(
            !line.contains("Op::"),
            "main.rs:{} が核の変更を直に作る(Headless か kmb_session::edit の関数を使う): {line}",
            n + 1
        );
    }
}

/// どの操作も ID で動かせ、操作の引数は宣言で確かめる(打ち間違いを黙って捨てて既定で動かない)。
#[test]
fn any_action_runs_by_id_with_declared_arguments() {
    let d = temp_dir("run");
    ok(&d, &["sample", "a.json"]);
    let list = ok(&d, &["actions", "a.json"]);
    assert!(list.contains("io.export_stl\t"), "{list}");
    assert!(list.contains("  tolerance(長さ mm、既定 0.01)"), "{list}");
    let json: Value = serde_json::from_str(&ok(&d, &["actions", "a.json", "--json"])).unwrap();
    let stl = json
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "io.export_stl")
        .unwrap();
    assert_eq!(stl["args"][1]["key"], "tolerance");
    assert_eq!(stl["args"][1]["default"], 0.01);

    // 打ち間違いは誤り(使える鍵を挙げる)
    let (done, _, err) = jc(
        &d,
        &[
            "run",
            "a.json",
            "io.export_stl",
            "--arg",
            "tolerence=0.1",
            "--out",
            "x.stl",
        ],
    );
    assert!(
        !done && err.contains("引数 tolerence はありません"),
        "{err}"
    );
    assert!(err.contains("tolerance"), "{err}");
    assert!(!d.join("x.stl").exists());
    // 単位つきの式は mm に直る(0.002 in = 0.0508 mm は、数で 0.0508 を渡したのと同じ網)
    ok(
        &d,
        &[
            "run",
            "a.json",
            "io.export_stl",
            "--arg",
            "tolerance=0.002 in",
            "--out",
            "in.stl",
        ],
    );
    ok(
        &d,
        &[
            "run",
            "a.json",
            "io.export_stl",
            "--arg",
            "tolerance=0.0508",
            "--out",
            "mm.stl",
        ],
    );
    let size = |f: &str| std::fs::metadata(d.join(f)).unwrap().len();
    assert_eq!(size("in.stl"), size("mm.stl"));
    assert!(size("in.stl") > 84);

    // 文書を変える操作は保存する(足した機能の ID を出す)
    let added = ok(
        &d,
        &["run", "a.json", "cad.add.sketch", "--arg", "plane=yz"],
    );
    let id: u64 = added
        .trim()
        .trim_start_matches('F')
        .parse()
        .expect("足した機能の ID(F16 の形)");
    let doc = read(&d.join("a.json"));
    let f = doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == id)
        .unwrap();
    assert_eq!(f["data"]["plane"]["kind"], "yz", "{f}");

    // 使えない操作は理由を返して何もしない
    let (done, _, err) = jc(
        &d,
        &["run", "a.json", "cad.sketch.export_dxf", "--out", "s.dxf"],
    );
    assert!(!done && err.contains("今使えません"), "{err}");
    std::fs::remove_dir_all(&d).ok();
}

/// `--component` を付けて作ると、作ったボディがその部品に入る(画面で部品をアクティブにして
/// から作るのと同じ)。無い部品は誤り。
#[test]
fn a_feature_made_with_component_goes_into_that_component() {
    let d = temp_dir("component");
    ok(&d, &["sample", "a.json"]);
    ok(
        &d,
        &[
            "run",
            "a.json",
            "asm.new_component",
            "--arg",
            "name=空",
            "--arg",
            "selection=[]",
        ],
    );
    let doc = read(&d.join("a.json"));
    let c = doc["components"][0]["id"].as_u64().unwrap();
    let pocket = doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "ポケット")
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let data = format!(r#"{{"body": {pocket}, "copy": true, "z": "50 mm"}}"#);
    let (done, _, _) = jc(
        &d,
        &[
            "feature",
            "add",
            "a.json",
            "solid.move@1",
            &data,
            "--component",
            "9999",
        ],
    );
    assert!(!done, "無い部品は誤り");
    let c_arg = c.to_string();
    let made: u64 = ok(
        &d,
        &[
            "feature",
            "add",
            "a.json",
            "solid.move@1",
            &data,
            "--component",
            &c_arg,
        ],
    )
    .trim()
    .trim_start_matches('F')
    .parse()
    .unwrap();
    let doc = read(&d.join("a.json"));
    assert_eq!(doc["components"][0]["bodies"], serde_json::json!([made]));
}

#[test]
fn obj_export_writes_indexed_triangles_without_a_material_file() {
    let d = temp_dir("obj-indexed-triangles");
    let document = format!("part.{}", kmb_core::DOCUMENT_EXTENSION);
    ok(&d, &["sample", &document]);
    let result = Command::new(env!("CARGO_BIN_EXE_kmb"))
        .args([
            "run",
            &document,
            "io.export_obj",
            "--arg",
            "unit=cm",
            "--out",
            "part.obj",
        ])
        .current_dir(&d)
        .env("XDG_DATA_HOME", d.join("data"))
        .env("APPDATA", d.join("data"))
        .env(kmb_session::logging::ENV, "info")
        .output()
        .expect("OBJ の書き出しを起動できる");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let logs = kmb_session::logging::log_dir(&d.join("data").join(kmb_session::DATA_DIR_NAME));
    let logs = std::fs::read_dir(logs)
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        logs.contains("OBJ を書き出す: ボディ 1 個、三角形") && logs.contains("単位 cm"),
        "OBJ のボディ数・三角形数・単位をログに残す: {logs}"
    );
    let text = std::fs::read_to_string(d.join("part.obj")).unwrap();
    let mut vertices: Vec<[f64; 3]> = Vec::new();
    let mut normals: Vec<[f64; 3]> = Vec::new();
    let mut faces = Vec::new();
    let mut names = Vec::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("v") | Some("vn") => {
                let values = words.map(|v| v.parse::<f64>().unwrap()).collect::<Vec<_>>();
                assert_eq!(values.len(), 3);
                assert!(values.iter().all(|x| x.is_finite()));
                let point = [values[0], values[1], values[2]];
                if line.starts_with("vn ") {
                    normals.push(point);
                } else {
                    vertices.push(point);
                }
            }
            Some("o") => names.push(words.collect::<Vec<_>>().join(" ")),
            Some("f") => faces.push(
                words
                    .map(|v| {
                        let (vertex, normal) = v.split_once("//").unwrap();
                        (
                            vertex.parse::<usize>().unwrap(),
                            normal.parse::<usize>().unwrap(),
                        )
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => {}
        }
    }
    assert!(!vertices.is_empty() && !normals.is_empty() && !faces.is_empty() && !names.is_empty());
    let mut volume = 0.;
    for face in faces {
        assert_eq!(face.len(), 3);
        for &(v, n) in &face {
            assert!((1..=vertices.len()).contains(&v));
            assert!((1..=normals.len()).contains(&n));
        }
        let [a, b, c] = [
            vertices[face[0].0 - 1],
            vertices[face[1].0 - 1],
            vertices[face[2].0 - 1],
        ];
        volume += (a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]))
            / 6.;
    }
    assert!(volume > 0., "外向きの三角形で正の体積になる: {volume}");
    assert!(!std::fs::read_dir(&d).unwrap().any(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "mtl")
    }));
    std::fs::remove_dir_all(&d).unwrap();
}
