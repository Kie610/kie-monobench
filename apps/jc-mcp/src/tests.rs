use super::*;
use crate::server::MeasureTarget;
use serde_json::{Value, json};

fn temp_root(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("jc-mcp-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_session_edits_the_part_and_rebuilds_the_program() {
    let root = temp_root("edit");
    let mut s = Session::new(root.clone());
    let st = s.sample(true, false).unwrap();
    assert_eq!(st["errors"], 0, "{st}");
    let before = s.mass().unwrap()[0]["max"][0].as_f64().unwrap();
    let st = s.set_param("width", "70 mm").unwrap();
    assert_eq!(st["errors"], 0, "{st}");
    let after = s.mass().unwrap()[0]["max"][0].as_f64().unwrap();
    assert!((after - before - 10.0).abs() < 1e-6, "{before} → {after}");
    let g = s.gcode(None, Some("part.nc"), None).unwrap();
    assert!(root.join("part.nc").exists(), "{g}");
    assert!(g["tools"] == json!([1, 2]), "{g}");
    s.save(Some("part.json"), false).unwrap();
    assert!(
        s.save(Some("part.nc"), false).is_err(),
        "既にあるファイルは上書きしない"
    );
    let mut t = Session::new(root.clone());
    let st = t.open("part.json").unwrap();
    assert_eq!(st["errors"], 0);
    assert!(
        st["params"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "width" && p["expr"] == "70 mm")
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_broken_reference_is_reported_with_its_reason() {
    let root = temp_root("broken");
    let mut s = Session::new(root.clone());
    s.sample(true, false).unwrap();
    // 形状の側のポケット(押し出しの切り取り)は、加工の側と同じ名前なので ID で探す
    let cut = s.status()["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "solid.extrude@1" && f["name"] == "ポケット")
        .map(|f| f["id"].as_u64().unwrap());
    let cut = cut.unwrap_or_else(|| panic!("切り取りが見つからない: {}", s.status()));
    let st = s.suppress(cut, true).unwrap();
    let op = st["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "cam.pocket2d@1" && f["name"] == "ポケット")
        .unwrap();
    assert_eq!(op["status"], "error", "{st}");
    assert!(
        op["reason"].as_str().unwrap().contains("参照が切れた"),
        "{op}"
    );
    assert!(s.gcode(None, None, None).is_err());
    let st = s.undo().unwrap();
    assert_eq!(st["errors"], 0, "{st}");
    std::fs::remove_dir_all(&root).ok();
}

// 確かめること:
// - 作業フォルダの外の道は断り、中には書ける
// - 許したフォルダは使え、外を指すリンクは断る
// 確かめること:
// - どの操作も、ファイルを読む引数は `Arg::file` で宣言している(鍵の名前がパスらしい文字の引数は無い)
// - ファイルの引数に作業フォルダの外を渡すと、操作を動かさずに断る(絵を貼る・下絵・フォントなど)
// - 中のパスは通る(無いファイルなら、操作が読めないと言う)
#[test]
fn every_file_arg_of_every_action_stays_inside_the_allowed_folders() {
    let root = temp_root("file-args");
    let outside = temp_root("file-args-outside");
    std::fs::write(outside.join("secret.png"), b"x").unwrap();
    let mut s = Session::new(root.clone());
    s.sample(false, false).unwrap();
    let actions = s.work.actions(None);
    let mut checked = 0;
    for a in &actions {
        for arg in &a.args {
            let pathlike = ["path", "file", "dir", "folder"]
                .iter()
                .any(|w| arg.key.contains(w));
            assert!(
                !(pathlike && arg.kind == jc_core::ArgKind::Text),
                "{} の {} はファイルのパスなら Arg::file で宣言する",
                a.id,
                arg.key
            );
            if arg.kind != jc_core::ArgKind::File {
                continue;
            }
            let secret = outside.join("secret.png").display().to_string();
            let up = format!(
                "../{}/secret.png",
                outside.file_name().unwrap().to_string_lossy()
            );
            for p in [secret.as_str(), up.as_str()] {
                let e = s
                    .run_action_at(&a.id, None, None, &json!({ arg.key.clone(): p }), None)
                    .unwrap_err();
                assert!(e.message.contains("外"), "{} の {}: {e}", a.id, arg.key);
            }
            checked += 1;
        }
    }
    assert!(
        checked >= 3,
        "ファイルの引数を持つ操作が {checked} 個しか無い"
    );
    let e = s
        .run_action_at(
            "cad.canvas_insert",
            None,
            None,
            &json!({"path": "none.png"}),
            None,
        )
        .unwrap_err();
    assert!(e.message.contains("読めません"), "{e}");
    for d in [root, outside] {
        std::fs::remove_dir_all(&d).ok();
    }
}

#[test]
fn paths_outside_the_root_and_allowed_folders_are_refused() {
    let root = temp_root("allow");
    let extra = temp_root("allow-extra");
    let outside = temp_root("allow-outside");
    let mut s = Session::new(root.clone());
    s.sample(false, false).unwrap();
    let e = s
        .export("../escape.stl", None, None, None, false)
        .unwrap_err();
    assert!(e.message.contains("外"), "{e}");
    assert!(s.open("/etc/hostname").is_err());
    s.export("part.stl", None, None, None, false).unwrap();
    assert!(root.join("part.stl").exists());
    // 許していないフォルダには書けない。許せば書ける
    let target = extra.join("part.stl").display().to_string();
    assert!(
        s.export(&target, None, None, None, false)
            .unwrap_err()
            .message
            .contains("外")
    );
    s.allow(&extra).unwrap();
    s.export(&target, None, None, None, false).unwrap();
    assert!(extra.join("part.stl").exists());
    #[cfg(unix)]
    {
        // 作業フォルダの中のリンクでも、指す先が外なら断る(行き先のないリンクも)
        std::fs::write(outside.join("doc.json"), "{}").unwrap();
        std::os::unix::fs::symlink(outside.join("doc.json"), root.join("link.json")).unwrap();
        let e = s.open("link.json").unwrap_err();
        assert!(e.message.contains("外"), "{e}");
        std::os::unix::fs::symlink(outside.join("none.stl"), root.join("dangling.stl")).unwrap();
        assert!(s.export("dangling.stl", None, None, None, false).is_err());
        assert!(!outside.join("none.stl").exists());
        // リンクのフォルダ越しに外へ出るのも断る
        std::os::unix::fs::symlink(&outside, root.join("out")).unwrap();
        assert!(
            s.export("out/escape.stl", None, None, None, false)
                .unwrap_err()
                .message
                .contains("外")
        );
        assert!(!outside.join("escape.stl").exists());
    }
    for d in [root, extra, outside] {
        std::fs::remove_dir_all(&d).ok();
    }
}

#[test]
fn crash_reports_are_listed_from_the_data_folder_only() {
    let root = temp_root("crash");
    let data = temp_root("crash-data");
    let s = Session::new(root.clone());
    assert!(s.crash_reports(None).is_err());
    let s = s.with_data_dir(data.clone());
    let dir = jc_session::crash::crash_dir(&data);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("crash-1-1.json"),
        r#"{"kind":"panic","app":"jc-mcp","version":"0.1.0","time":5,"message":"例"}"#,
    )
    .unwrap();
    let list = s.crash_reports(None).unwrap();
    assert_eq!(list[0]["name"], "crash-1-1.json", "{list}");
    assert_eq!(list[0]["message"], "例");
    let one = s.crash_reports(Some("crash-1-1.json")).unwrap();
    assert_eq!(one["kind"], "panic");
    assert!(s.crash_reports(Some("../../etc/passwd")).is_err());
    for d in [root, data] {
        std::fs::remove_dir_all(&d).ok();
    }
}

/// 実際の MCP の通信(初期化、道具の一覧、道具の呼び出し)を、メモリ上の管でつないで確かめる。
#[tokio::test(flavor = "multi_thread")]
async fn the_server_speaks_mcp() {
    use rmcp::ServiceExt;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let root = temp_root("wire");
    let (client, server_io) = tokio::io::duplex(1 << 20);
    let server = Server::new(Session::new(root.clone()));
    let handle = tokio::spawn(async move {
        let running = server.serve(server_io).await.unwrap();
        let _ = running.waiting().await;
    });
    let (r, mut w) = tokio::io::split(client);
    let mut lines = BufReader::new(r).lines();
    let mut send = async |v: Value| {
        w.write_all(format!("{v}\n").as_bytes()).await.unwrap();
    };
    send(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }}),
    )
    .await;
    let init: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(
        init["result"]["serverInfo"]["name"], "kie-monobench",
        "{init}"
    );
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await;
    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    let list: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for n in [
        "status",
        "set_param",
        "gcode",
        "simulate",
        "mass",
        "set_material",
        "topology",
        "measure",
        "export",
        "crash_reports",
    ] {
        assert!(names.contains(&n), "{n} がない: {names:?}");
    }
    send(
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
            "name": "sample", "arguments": {"cam": false}
        }}),
    )
    .await;
    let r: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(r["result"]["isError"], false, "{r}");
    send(
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {
            "name": "set_param", "arguments": {"name": "width", "expr": "-5 mm"}
        }}),
    )
    .await;
    let r: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    let st: Value = serde_json::from_str(text).unwrap();
    assert!(
        st["errors"].as_u64().unwrap() > 0,
        "負の幅は誤りになる: {st}"
    );
    send(
        json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {
            "name": "mass", "arguments": {}
        }}),
    )
    .await;
    let r: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(
        r["result"]["isError"], true,
        "誤りのある文書の質量は出さない: {r}"
    );
    drop(w);
    handle.abort();
    std::fs::remove_dir_all(&root).ok();
}

/// 採用分 10-24 m3: 壊れたクライアントが改行の無い大きすぎる送り物を流しても、記憶を食い尽くさず
/// 接続を終える。上限より小さい送り物はそのまま通る。
#[tokio::test(flavor = "multi_thread")]
async fn an_oversized_message_ends_the_connection_instead_of_eating_memory() {
    use crate::line_cap::LineCap;
    use rmcp::ServiceExt;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let root = temp_root("oversized");
    let (client, server_io) = tokio::io::duplex(1 << 16);
    let (sr, sw) = tokio::io::split(server_io);
    let server = Server::new(Session::new(root.clone()));
    // 試験は小さな上限で確かめる(本物は MAX_LINE)
    let handle = tokio::spawn(async move {
        let running = server.serve((LineCap::new(sr, 4096), sw)).await.unwrap();
        let _ = running.waiting().await;
    });
    let (r, mut w) = tokio::io::split(client);
    let mut lines = BufReader::new(r).lines();
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": {"name": "test", "version": "0"}
    }});
    w.write_all(format!("{init}\n").as_bytes()).await.unwrap();
    let got: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(
        got["result"]["serverInfo"]["name"], "kie-monobench",
        "{got}"
    );
    // 改行の無いまま上限を超えて流すと、サーバは読むのをやめて終わる(終われば管が閉じ、書き手も止まる)
    tokio::spawn(async move {
        let chunk = vec![b'x'; 1024];
        while w.write_all(&chunk).await.is_ok() {}
    });
    handle.await.unwrap();
    std::fs::remove_dir_all(&root).ok();
}

// 同じ作業フォルダで、読み込みを取り消して続けて確かめる:
// - 作業フォルダの中の STEP は読み込め、外のものは断る
// - 読み込みながら立てられ、1 回で取り消せる
#[test]
fn a_step_file_inside_the_root_is_imported_stood_up_and_undone_and_one_outside_is_refused() {
    let root = temp_root("step");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/jc-kernel-truck/testdata/reference.step");
    std::fs::copy(&fixture, root.join("part.step")).unwrap();
    let mut s = Session::new(root.clone());
    let st = s.import_step("part.step", false).unwrap();
    assert_eq!(st["errors"], 0, "{st}");
    let v = s.mass().unwrap()[0]["volume"].as_f64().unwrap();
    assert!((v - 22806.876).abs() / 22806.876 < 2e-4, "{v}");
    assert!(
        s.import_step(fixture.to_str().unwrap(), false).is_err(),
        "作業フォルダの外は読まない"
    );
    let st = s.undo().unwrap();
    assert_eq!(st["features"].as_array().unwrap().len(), 0, "{st}");

    let st = s.import_step("part.step", true).unwrap();
    assert_eq!(st["errors"], 0, "{st}");
    let id = st["added"].as_u64().unwrap();
    assert_eq!(s.feature_data(id).unwrap()["data"]["stand_up"], true);
    // 回しても体積は変わらない
    let v = s.mass().unwrap()[0]["volume"].as_f64().unwrap();
    assert!((v - 22806.876).abs() / 22806.876 < 2e-4, "{v}");
    let st = s.undo().unwrap();
    assert_eq!(
        st["features"].as_array().unwrap().len(),
        0,
        "読み込みと立てるは 1 回で戻る: {st}"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn design_properties_are_set_read_and_undone() {
    let mut s = Session::new(temp_root("props"));
    s.sample(false, false).unwrap();
    let set: std::collections::BTreeMap<String, String> = [
        ("title".to_string(), "台座".to_string()),
        ("material".to_string(), "A5052".to_string()),
        ("検査".to_string(), "全数".to_string()),
    ]
    .into();
    let r = s.properties(&set).unwrap();
    assert_eq!(r["properties"]["title"], "台座", "{r}");
    assert_eq!(r["properties"]["material"], "A5052", "{r}");
    assert!(
        r.to_string().contains("全数"),
        "自分で足した項目も返す: {r}"
    );
    s.undo().unwrap();
    let r = s.properties(&Default::default()).unwrap();
    assert_eq!(r["properties"]["title"], "", "{r}");
    // 文書の長さの単位(候補 19)。読めない単位は断り、何も変えない
    assert_eq!(r["properties"]["units"], "mm", "{r}");
    let set: std::collections::BTreeMap<String, String> =
        [("units".to_string(), "inch".to_string())].into();
    let r = s.properties(&set).unwrap();
    assert_eq!(r["properties"]["units"], "in", "{r}");
    let set: std::collections::BTreeMap<String, String> =
        [("units".to_string(), "yd".to_string())].into();
    assert!(s.properties(&set).is_err());
}

#[test]
fn saved_versions_are_listed_named_and_restored() {
    let root = temp_root("versions");
    let _ = std::fs::remove_dir_all(root.join(".jc"));
    let mut s = Session::new(root.clone());
    s.sample(false, false).unwrap();
    // タグ・ジャンル・プロジェクトは設計の属性に入る
    let set: std::collections::BTreeMap<String, String> = [
        ("tags".to_string(), "治具, アルミ、治具".to_string()),
        ("genre".to_string(), "治具".to_string()),
    ]
    .into();
    let r = s.properties(&set).unwrap();
    assert_eq!(r["properties"]["tags"], json!(["治具", "アルミ"]), "{r}");
    assert_eq!(r["properties"]["genre"], "治具", "{r}");
    s.save(Some("part.json"), true).unwrap();
    let width = |s: &Session| s.status()["params"].to_string().contains("70 mm");
    s.set_param("width", "70 mm").unwrap();
    assert!(width(&s));
    s.save(None, false).unwrap();
    let v = s.versions(None, "").unwrap();
    let list = v["versions"].as_array().unwrap();
    assert_eq!(list.len(), 2, "{v}");
    assert_eq!(list[0]["number"], 2);
    assert!(root.join(".jc/history/part.json/index.json").exists());
    // 縮小図も残る
    assert_eq!(list[0]["thumbnail"], true, "{v}");
    // 節目の名前を付ける
    let v = s.versions(Some("1"), "初めの形").unwrap();
    assert_eq!(v["versions"][1]["milestone"], "初めの形", "{v}");
    // 版 1 に戻すと幅は元に戻り、戻したことが版 3 として残る
    let st = s.restore_version("1").unwrap();
    assert_eq!(st["restored"]["number"], 1, "{st}");
    assert!(!width(&s));
    let v = s.versions(None, "").unwrap();
    assert_eq!(v["versions"][0]["number"], 3, "{v}");
    assert_eq!(v["versions"][0]["note"], "版 1 に戻した", "{v}");
    assert!(s.restore_version("99").is_err());
}

#[test]
fn types_describe_the_data_of_each_feature() {
    let s = Session::new(temp_root("types"));
    let t = s.types();
    let ex = t
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["type_id"] == "solid.extrude@1")
        .unwrap();
    assert_eq!(ex["category"], "cad.solid");
    assert!(
        ex["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["key"] == "distance" && f["kind"] == "expr"),
        "{ex}"
    );
}

#[test]
fn module_actions_are_listed_and_run_like_on_the_screen() {
    let root = temp_root("actions");
    let mut s = Session::new(root.clone());
    s.sample(true, false).unwrap();
    let setup = s.status()["setups"][0]["id"].as_u64().unwrap();
    let list = s.actions(Some(setup));
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    for id in [
        "cam.export_gcode",
        "cam.simulate",
        "io.export_stl",
        "io.export_step",
    ] {
        assert!(ids.contains(&id), "{id} がない: {ids:?}");
    }
    let r = s
        .run_action_at(
            "cam.setup_sheet",
            Some(setup),
            None,
            &Value::Null,
            Some("sheet.md"),
        )
        .unwrap();
    assert!(root.join("sheet.md").exists(), "{r}");
    let r = s
        .run_action_at("cam.add.face", Some(setup), None, &Value::Null, None)
        .unwrap();
    assert_eq!(r["added"].as_array().unwrap().len(), 1, "{r}");
    let g = s.gcode(Some(setup), None, None).unwrap();
    assert!(
        g["gcode"].as_str().unwrap().contains("M30"),
        "{}",
        g["lines"]
    );
    assert!(
        s.run_action_at("no.such", None, None, &Value::Null, None)
            .is_err()
    );
    let sim = s.simulate(Some(setup), Some(0.5)).unwrap();
    let eng = sim["engagement"].as_array().unwrap();
    assert!(
        !eng.is_empty() && eng.iter().all(|e| e["max_deg"].as_f64().unwrap() <= 360.0),
        "{sim}"
    );
    s.export("part.step", None, None, None, false).unwrap();
    assert!(root.join("part.step").exists());
    // ボディごと: 名前の後ろにボディの名前を付ける(M55)
    let r = s.export("each.stl", None, None, None, true).unwrap();
    assert!(root.join("each-ボディ1.stl").exists(), "{r}");
    assert!(!root.join("each.stl").exists());
    assert!(
        s.export("part.obj", None, None, None, false)
            .unwrap_err()
            .message
            .contains(".stl")
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn text_is_placed_on_a_sketch_point_by_the_same_action_as_the_screen() {
    let mut s = Session::new(temp_root("text"));
    let st = s
        .add_feature(
            "cad.sketch@1",
            Some("点"),
            json!({"entities": [{"type": "point", "id": 1, "x": 12.0, "y": 8.0}]}),
        )
        .unwrap();
    let sketch = st["added"].as_u64().unwrap();
    let ids: Vec<String> = s
        .actions(Some(sketch))
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["disabled"].is_null())
        .map(|a| a["id"].as_str().unwrap().to_string())
        .collect();
    assert!(ids.contains(&"cad.text_at_point".to_string()), "{ids:?}");
    let r = s
        .run_action_at("cad.text_at_point", Some(sketch), None, &Value::Null, None)
        .unwrap();
    assert_eq!(r["errors"], 0, "{r}");
    let text = r["added"][0].as_u64().unwrap();
    let f = s.feature_data(text).unwrap();
    assert_eq!(f["data"]["anchor"], sketch, "{f}");
    assert_eq!(f["data"]["align"], "center", "{f}");
}

#[test]
fn text_is_laid_along_a_sketch_circle_by_the_same_action_as_the_screen() {
    let mut s = Session::new(temp_root("path"));
    let st = s
        .add_feature(
            "cad.sketch@1",
            Some("円"),
            json!({"entities": [
                {"type": "point", "id": 1, "x": 0.0, "y": 0.0},
                {"type": "circle", "id": 2, "center": 1, "radius": 25.0, "construction": true}
            ]}),
        )
        .unwrap();
    let sketch = st["added"].as_u64().unwrap();
    let r = s
        .run_action_at("cad.text_on_path", Some(sketch), None, &Value::Null, None)
        .unwrap();
    assert_eq!(r["errors"], 0, "{r}");
    let text = r["added"][0].as_u64().unwrap();
    let f = s.feature_data(text).unwrap();
    assert_eq!(f["data"]["path"], sketch, "{f}");
    assert_eq!(f["data"]["reverse"], true, "{f}");
}

#[test]
fn settings_and_the_undo_history_are_reachable() {
    let mut s = Session::new(temp_root("settings"));
    let all = s.settings();
    let machine = |v: &Value| {
        v.as_array()
            .unwrap()
            .iter()
            .find(|g| g["id"] == "cam.machine")
            .cloned()
            .expect("機械の設定")
    };
    let m = machine(&all);
    assert_eq!(m["value"], m["default"]);
    assert!(m["notes"].to_string().contains("Carvera Air"), "{m}");
    let r = s
        .set_settings("cam.machine", &json!({"rapid_feed": 1234.0}))
        .unwrap();
    assert_eq!(machine(&r)["value"]["rapid_feed"], 1234.0);
    // 正しくない値と知らない設定は断り、何も変えない
    assert!(
        s.set_settings("cam.machine", &json!({"rapid_feed": -1.0}))
            .is_err()
    );
    assert!(s.set_settings("no.such", &json!({})).is_err());
    assert_eq!(machine(&s.settings())["value"]["rapid_feed"], 1234.0);
    // 取り消しの履歴に、変えたパラメータと値が出る
    s.sample(false, false).unwrap();
    s.set_param("width", "70 mm").unwrap();
    let h = s.history();
    assert_eq!(
        h["done"].as_array().unwrap().last().unwrap(),
        "width を 70 mm に変更"
    );
    s.undo().unwrap();
    assert_eq!(s.history()["undone"], json!(["width を 70 mm に変更"]));
}

// 同じ見本と面の一覧で確かめる(文書を変える穴は最後):
// - 面と辺を一覧にし、画面と同じように測れる
// - 曲面への穴は、理由つきですぐ断る
#[test]
fn faces_and_edges_are_listed_and_measured_and_a_hole_on_a_curved_face_is_refused() {
    let root = temp_root("measure");
    let mut s = Session::new(root.clone());
    s.sample(false, false).unwrap();
    let topo = s.topology(None).unwrap();
    let body = &topo["bodies"][0];
    let id = body["id"].as_u64().unwrap();
    let faces = body["faces"].as_array().unwrap();
    let plane = |nz: f64| {
        faces
            .iter()
            .filter(|f| {
                f["shape"]["type"] == "plane"
                    && (f["shape"]["value"]["normal"][2].as_f64().unwrap() - nz).abs() < 1e-9
            })
            // 下向きはいちばん低い面、上向きはいちばん高い面(ボディの外形の高さの両端)
            .min_by(|a, b| {
                let z = |f: &&Value| f["shape"]["value"]["point"][2].as_f64().unwrap();
                (nz * z(b)).total_cmp(&(nz * z(a)))
            })
            .unwrap_or_else(|| panic!("法線 z={nz} の平面が無い: {body}"))
    };
    let (bottom, top) = (plane(-1.0), plane(1.0));
    let thickness = top["shape"]["value"]["point"][2].as_f64().unwrap()
        - bottom["shape"]["value"]["point"][2].as_f64().unwrap();
    let mass = s.mass().unwrap()[0].clone();
    let want = mass["max"][2].as_f64().unwrap() - mass["min"][2].as_f64().unwrap();
    assert!((thickness - want).abs() < 1e-3, "{thickness} {want}");

    // 面を 1 つ: 面積と向き、ボディの体積(画面の窓と同じ行)
    let one = s
        .measure(
            &[MeasureTarget {
                body: Some(id),
                face: Some(bottom["name"].as_str().unwrap().into()),
                edge: None,
                center: None,
                point: None,
            }],
            None,
        )
        .unwrap();
    let rows = one["rows"].as_array().unwrap();
    let row = |rows: &[Value], name: &str| {
        rows.iter()
            .find(|r| r["name"] == name)
            .map(|r| r["value"].as_str().unwrap().to_string())
            .unwrap_or_else(|| panic!("{name} の行が無い: {rows:?}"))
    };
    assert_eq!(
        row(rows, "ボディの体積"),
        format!("{:.3} mm³", mass["volume"].as_f64().unwrap())
    );
    assert_eq!(one["shapes"][0]["type"], "plane");

    // 面を 2 つ: 平行な面の間隔は板の厚さ
    let face = |f: &Value| MeasureTarget {
        body: Some(id),
        face: Some(f["name"].as_str().unwrap().into()),
        edge: None,
        center: None,
        point: None,
    };
    let two = s.measure(&[face(bottom), face(top)], None).unwrap();
    assert_eq!(
        row(two["rows"].as_array().unwrap(), "平行な面の間隔"),
        format!("{want:.3} mm"),
        "{two}"
    );

    // 円の辺を 1 つ: 直径。点と点: 距離
    let circle = body["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["shape"]["type"] == "circle")
        .unwrap_or_else(|| panic!("円の辺が無い: {body}"));
    let r = circle["shape"]["value"]["radius"].as_f64().unwrap();
    let c = s
        .measure(
            &[MeasureTarget {
                body: Some(id),
                face: None,
                edge: Some(circle["index"].as_u64().unwrap() as usize),
                center: None,
                point: None,
            }],
            None,
        )
        .unwrap();
    assert_eq!(
        row(c["rows"].as_array().unwrap(), "直径"),
        format!("{:.3} mm", 2.0 * r)
    );
    let pt = |p: [f64; 3]| MeasureTarget {
        body: None,
        face: None,
        edge: None,
        center: None,
        point: Some(p),
    };
    let d = s
        .measure(&[pt([0.0, 0.0, 0.0]), pt([3.0, 4.0, 0.0])], None)
        .unwrap();
    assert_eq!(row(d["rows"].as_array().unwrap(), "最短の距離"), "5.000 mm");

    // 指し方の誤りは理由を返す
    assert!(s.measure(&[], None).is_err());
    assert!(s.measure(&[pt([0.0; 3])], Some("far")).is_err());
    let e = s
        .measure(
            &[MeasureTarget {
                body: Some(id),
                face: Some("無い面".into()),
                edge: None,
                center: None,
                point: None,
            }],
            None,
        )
        .unwrap_err();
    assert!(e.message.contains("無い面"), "{e}");
    assert!(s.topology(Some(999_999)).is_err());

    // 曲面への穴は理由つきですぐ断る(文書を変えるので最後の段)。
    // 画面の手順は曲面のクリックを断るが、CLI・MCP には面を渡す道が無く、いつも「面を選んでいません」だった
    let named = |kind: &str| {
        faces
            .iter()
            .find(|f| f["shape"]["type"] == kind)
            .and_then(|f| f["name"].as_str())
            .unwrap_or_else(|| panic!("{kind} の面が無い: {body}"))
            .to_string()
    };
    let args = |face: &str| serde_json::json!({"feature": id, "face": face, "diameter": 3.0});
    let e = s
        .run_action_at(
            "cad.hole_on_face",
            None,
            None,
            &args(&named("cylinder")),
            None,
        )
        .unwrap_err();
    assert!(e.message.contains("曲面"), "{e}");
    let e = s
        .run_action_at("cad.hole_on_face", None, None, &args("f999:end"), None)
        .unwrap_err();
    assert!(e.message.contains("面 f999:end がありません"), "{e}");
    let r = s
        .run_action_at("cad.hole_on_face", None, None, &args(&named("plane")), None)
        .unwrap();
    assert_eq!(r["added"].as_array().unwrap().len(), 2, "{r}");
    std::fs::remove_dir_all(&root).ok();
}

// 同じ見本で、文書を変えない段から順に確かめる:
// - 受け付けない手(開く・保存・取り消し・入れ子・知らない道具・読めない引数)があれば 1 手目も動かさない
// - 途中で失敗すると始めの形に戻し、失敗した手を言い、戻した手はやり直せない
// - 順に流し、2 手の変更は 1 回の取り消し単位になる
#[test]
fn a_batch_refuses_bad_steps_rolls_back_on_failure_and_undoes_as_one() {
    let mut s = Session::new(temp_root("batch"));
    s.sample(false, false).unwrap();
    // --- 受け付けない手があれば何もしない
    let start = s.status();
    for (bad, why) in [
        (
            json!({"tool": "open", "args": {"path": "x.json"}}),
            "入れ替える",
        ),
        (json!({"tool": "save"}), "保存"),
        (json!({"tool": "undo"}), "取り消し"),
        (
            json!({"tool": "batch", "args": {"steps": []}}),
            "batch の中",
        ),
        (json!({"tool": "no_such"}), "知らない道具"),
        (
            json!({"tool": "set_param", "args": {"name": "width"}}),
            "引数を読めません",
        ),
    ] {
        let steps = serde_json::from_value(json!([
            {"tool": "set_param", "args": {"name": "width", "expr": "70 mm"}},
            bad,
        ]))
        .unwrap();
        let e = s.batch(steps).unwrap_err();
        let v: Value = serde_json::from_str(&e.message).unwrap();
        assert_eq!(v["failed"]["step"], 2, "{v}");
        assert!(v["failed"]["error"].as_str().unwrap().contains(why), "{v}");
        assert_eq!(v["rolled_back"], 0, "{v}");
        assert_eq!(s.status()["params"], start["params"], "1 手目も動かさない");
    }

    // --- 途中で失敗すると始めの形に戻し、失敗した手を言う
    let start = s.status();
    let history = s.history();
    let last = start["features"].as_array().unwrap().last().unwrap()["id"].clone();
    let steps = serde_json::from_value(json!([
        {"tool": "set_param", "args": {"name": "width", "expr": "70 mm"}},
        {"tool": "suppress", "args": {"id": last, "suppressed": true}},
        {"tool": "remove_feature", "args": {"id": 9999}},
        {"tool": "set_param", "args": {"name": "height", "expr": "45 mm"}},
    ]))
    .unwrap();
    let e = s.batch(steps).unwrap_err();
    let v: Value = serde_json::from_str(&e.message).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["failed"]["step"], 3, "{v}");
    assert_eq!(v["failed"]["tool"], "remove_feature", "{v}");
    assert!(
        v["failed"]["error"].as_str().unwrap().contains("9999"),
        "{v}"
    );
    assert_eq!(v["rolled_back"], 2, "{v}");
    // 始めの形に戻り、戻した手はやり直せない
    assert_eq!(s.status()["params"], start["params"]);
    assert_eq!(s.status()["features"], start["features"]);
    assert_eq!(s.history()["done"], history["done"]);
    assert_eq!(s.history()["undone"], json!([]));

    // --- 順に流し、1 回で取り消せる
    let before = s.history()["done"].as_array().unwrap().len();
    let steps = serde_json::from_value(json!([
        {"tool": "set_param", "args": {"name": "width", "expr": "70 mm"}},
        {"tool": "set_param", "args": {"name": "height", "expr": "45 mm"}},
        {"tool": "mass"},
    ]))
    .unwrap();
    let r = s.batch(steps).unwrap();
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["changes"], 2, "{r}");
    assert_eq!(
        r["steps"][0],
        json!({"tool": "set_param"}),
        "要約は手ごとに返さない"
    );
    let max = &r["steps"][2]["result"][0]["max"];
    assert!((max[0].as_f64().unwrap() - 70.0).abs() < 1e-6, "{r}");
    // 2 手の変更は 1 回の取り消し単位
    let h = s.history();
    assert_eq!(h["done"].as_array().unwrap().len(), before + 1, "{h}");
    assert_eq!(
        h["done"].as_array().unwrap().last().unwrap(),
        "まとめて実行(3 手)"
    );
    s.undo().unwrap();
    let p = |s: &Session, n: &str| {
        s.status()["params"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == n)
            .unwrap()["expr"]
            .clone()
    };
    assert_ne!(p(&s, "width"), "70 mm");
    assert_ne!(p(&s, "height"), "45 mm");
}

// 寸法や値をいくつも直す batch は、形の作り直しを最後の 1 回にまとめる(10-20 s18)。
// ほかの道具の前では作り直してから動かすので、質量は直した後の形で測る
#[test]
fn a_batch_of_value_edits_recomputes_the_shape_once() {
    let mut s = Session::new(temp_root("batch-once"));
    s.sample(false, false).unwrap();
    let before = s.recomputes();
    let steps = serde_json::from_value(json!([
        {"tool": "set_param", "args": {"name": "width", "expr": "70 mm"}},
        {"tool": "set_param", "args": {"name": "height", "expr": "45 mm"}},
        {"tool": "set_param", "args": {"name": "thickness", "expr": "12 mm"}},
    ]))
    .unwrap();
    let r = s.batch(steps).unwrap();
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(s.recomputes() - before, 1, "作り直しは最後の 1 回");
    let p = s.status()["params"].clone();
    let w = p
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "width")
        .unwrap();
    assert_eq!(w["value"], "70 mm", "{p}");

    // 値の手の後に形を読む手があれば、その前に 1 回作り直す(合わせて 2 回)
    let before = s.recomputes();
    let steps = serde_json::from_value(json!([
        {"tool": "set_param", "args": {"name": "width", "expr": "80 mm"}},
        {"tool": "set_param", "args": {"name": "height", "expr": "50 mm"}},
        {"tool": "mass"},
        {"tool": "set_param", "args": {"name": "thickness", "expr": "10 mm"}},
    ]))
    .unwrap();
    let r = s.batch(steps).unwrap();
    assert_eq!(s.recomputes() - before, 2, "{r}");
    let max = &r["steps"][2]["result"][0]["max"];
    assert!((max[0].as_f64().unwrap() - 80.0).abs() < 1e-6, "{r}");

    // 途中で失敗したら、戻した形を 1 回だけ作り直す
    let start = s.status();
    let before = s.recomputes();
    let steps = serde_json::from_value(json!([
        {"tool": "set_param", "args": {"name": "width", "expr": "90 mm"}},
        {"tool": "set_value", "args": {"feature": 9999, "key": "distance", "expr": "3"}},
    ]))
    .unwrap();
    s.batch(steps).unwrap_err();
    assert_eq!(s.recomputes() - before, 1);
    assert_eq!(s.status()["params"], start["params"]);
}

#[test]
fn every_tool_is_known_to_the_batch() {
    // 道具を足した時に、batch の読み方を足し忘れない
    let names = crate::Server::tool_names();
    assert!(names.len() > 25, "{names:?}");
    for name in names {
        match crate::batch::parse(&name, json!({})) {
            Err(e) if e.contains("知らない道具") => panic!("batch が {name} を知らない"),
            _ => {}
        }
    }
}

#[test]
fn a_screenshot_is_a_png_of_the_part_and_can_mark_faces() {
    let root = temp_root("shot");
    let mut s = Session::new(root.clone());
    s.sample(false, false).unwrap();
    let arg = |v: Value| -> crate::server::ScreenshotArg { serde_json::from_value(v).unwrap() };
    let (png, info) = s
        .screenshot(&arg(json!({"width": 320, "height": 240})))
        .unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(info["view"], "iso", "{info}");
    assert_eq!(
        (info["width"].as_u64(), info["height"].as_u64()),
        (Some(320), Some(240))
    );
    assert!(info["px_per_mm"].as_f64().unwrap() > 1.0, "{info}");
    // 面を目立たせ、上から描いて書き出す
    let topo = s.topology(None).unwrap();
    let body = topo["bodies"][0]["id"].as_u64().unwrap();
    let face = topo["bodies"][0]["faces"][0]["name"]
        .as_str()
        .unwrap()
        .to_string();
    let (marked, info) = s
        .screenshot(&arg(json!({
            "view": "top", "faces": [{"body": body, "face": face}],
            "edges": [{"body": body, "edge": 0}], "out": "top.png"
        })))
        .unwrap();
    assert_eq!(
        std::fs::read(root.join("top.png")).unwrap(),
        marked,
        "{info}"
    );
    // 知らない視点・面・辺、片方だけの角度、外へのパスは断る
    for (bad, why) in [
        (json!({"view": "diagonal"}), "知りません"),
        (
            json!({"faces": [{"body": body, "face": "no_such"}]}),
            "no_such",
        ),
        (json!({"edges": [{"body": body, "edge": 99999}]}), "辺"),
        (json!({"yaw": 10.0}), "両方"),
        (json!({"out": "../x.png"}), "外"),
    ] {
        let e = s.screenshot(&arg(bad)).unwrap_err();
        assert!(e.contains(why), "{e}");
    }
    // batch の中では説明だけ返し、out に書く
    let steps = serde_json::from_value(json!([
        {"tool": "set_param", "args": {"name": "width", "expr": "80 mm"}},
        {"tool": "screenshot", "args": {"out": "wide.png", "width": 64, "height": 64}},
    ]))
    .unwrap();
    let r = s.batch(steps).unwrap();
    assert!(r["steps"][1]["result"]["written"].is_string(), "{r}");
    assert!(root.join("wide.png").exists());
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn base64_matches_the_standard_alphabet_and_padding() {
    assert_eq!(crate::session::base64(b""), "");
    assert_eq!(crate::session::base64(b"f"), "Zg==");
    assert_eq!(crate::session::base64(b"fo"), "Zm8=");
    assert_eq!(crate::session::base64(b"foo"), "Zm9v");
    assert_eq!(crate::session::base64(b"foobar"), "Zm9vYmFy");
    assert_eq!(crate::session::base64(&[0xff, 0xfe, 0xfd]), "//79");
}

#[test]
fn feature_values_are_listed_and_edited_and_param_views_filter_by_body() {
    let root = temp_root("views");
    let mut s = Session::new(root.clone());
    let st = s.sample(false, false).unwrap();
    // 機能の値: 板の押し出しの距離(thickness)が出る
    let values = st["values"].as_array().expect("機能の値");
    let plate = values
        .iter()
        .find(|v| v["key"] == "distance" && v["expr"] == "thickness")
        .unwrap_or_else(|| panic!("{values:?}"))
        .clone();
    let pf = plate["feature"].as_u64().unwrap();
    assert_eq!(plate["label"], "距離");
    assert_eq!(plate["value"], "10 mm");
    // 数だけなら今の単位を補う。式の誤りと知らない欄は断る
    let pocket = values
        .iter()
        .find(|v| v["key"] == "distance" && v["expr"] == "pocket_depth")
        .unwrap()["feature"]
        .as_u64()
        .unwrap();
    let st = s.set_value(pocket, "distance", "3").unwrap();
    let now = st["values"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["feature"] == pocket && v["key"] == "distance")
        .unwrap()
        .clone();
    assert_eq!(
        (now["expr"].as_str(), now["value"].as_str()),
        (Some("3 mm"), Some("3 mm"))
    );
    assert!(s.set_value(pocket, "distance", "3 +").is_err());
    assert!(s.set_value(pocket, "no_such", "3").is_err());
    // 知らない名前の式は入れずに断る(入れると機能が止まる)
    let e = s.set_value(pocket, "distance", "nosuch * 2").unwrap_err();
    assert!(e.message.contains("nosuch"), "{e}");
    assert!(s.set_value(pocket, "distance", "thickness / 2").is_ok());
    assert!(
        s.history()["done"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .as_str()
            .unwrap()
            .contains("距離")
    );
    // 板のボディの表: 板の距離・幅などが決まりで入る
    let edit = |s: &mut Session, v: Value| s.edit_param_view(&serde_json::from_value(v).unwrap());
    let v = edit(&mut s, json!({"action": "create", "body": pf})).unwrap();
    let id = v["id"].as_u64().unwrap();
    assert!(v["name"].as_str().unwrap().ends_with("のパラメータ"), "{v}");
    let rows = |v: &Value| -> Vec<String> {
        v["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["row"].as_str().unwrap().to_string())
            .collect()
    };
    let r = rows(&v);
    for want in ["width", "height", "thickness", &format!("{pf}:distance")] {
        assert!(r.iter().any(|x| x == want), "{want} が無い: {r:?}");
    }
    let all = rows(&s.param_views(Some(0)).unwrap());
    assert!(r.len() < all.len(), "絞り込む: {} / {}", r.len(), all.len());
    // 外す・入れる・名前を変える(どれも取り消せる)
    let v = edit(
        &mut s,
        json!({"action": "hide", "id": id, "rows": ["width"]}),
    )
    .unwrap();
    assert!(!rows(&v).contains(&"width".to_string()));
    let v = edit(
        &mut s,
        json!({"action": "add", "id": id, "rows": ["width", "hole"]}),
    )
    .unwrap();
    assert_eq!(&rows(&v)[..2], ["width", "hole"], "手で入れた行が先");
    edit(
        &mut s,
        json!({"action": "rename", "id": id, "name": "外形"}),
    )
    .unwrap();
    assert_eq!(s.status()["param_views"][0]["name"], "外形");
    // 表の中で式を直すと、元のパラメータが変わる(写しは無い)
    s.set_param("width", "66 mm").unwrap();
    let v = s.param_views(Some(id)).unwrap();
    assert_eq!(v["rows"][0]["expr"], "66 mm");
    // 保存して開き直しても残る(保存形式の今の版)
    s.save(Some("views.json"), false).unwrap();
    let text = std::fs::read_to_string(root.join("views.json")).unwrap();
    let version = format!("\"format_version\": {}", jc_core::FORMAT_VERSION);
    assert!(text.contains(&version) && text.contains("外形"));
    let mut t = Session::new(root.clone());
    t.open("views.json").unwrap();
    assert_eq!(
        rows(&t.param_views(Some(id)).unwrap())[..2],
        ["width", "hole"]
    );
    // 知らない行・知らない表・知らない操作は断る
    assert!(
        edit(
            &mut s,
            json!({"action": "add", "id": id, "rows": ["no_such"]})
        )
        .is_err()
    );
    assert!(
        edit(
            &mut s,
            json!({"action": "add", "id": 9999, "rows": ["width"]})
        )
        .is_err()
    );
    assert!(edit(&mut s, json!({"action": "explode", "id": id})).is_err());
    // 消すと表だけなくなり、取り消すと戻る
    let list = edit(&mut s, json!({"action": "remove", "id": id})).unwrap();
    assert_eq!(list, json!([]));
    s.undo().unwrap();
    assert_eq!(s.status()["param_views"][0]["id"], id);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_app_tool_reaches_a_running_screen_and_returns_its_picture() {
    let root = temp_root("app");
    let data = root.join("data");
    let roots = jc_session::roots::Roots::new(root.clone());
    let arg = |tool: &str| crate::server::AppArg {
        tool: tool.into(),
        args: json!({}),
    };
    // 画面が待ち受けていなければ、入れ方を返す
    let r = crate::server::call_app(Some(data.clone()), &roots, arg("status"));
    assert_eq!(r.is_error, Some(true));
    assert!(format!("{:?}", r.content).contains("AI との接続"));
    // 画面の代わり(絵を返す)
    let (tx, rx) = std::sync::mpsc::channel::<jc_session::live::Request>();
    let l = jc_session::live::Listener::start(&data, tx, || {}).unwrap();
    let screen = std::thread::spawn(move || {
        let r = rx.recv().unwrap();
        assert_eq!(r.tool, "screenshot");
        r.reply
            .send(Ok(
                json!({"png": jc_session::live::base64(b"\x89PNG"), "width": 1}),
            ))
            .unwrap();
    });
    let r = crate::server::call_app(Some(data.clone()), &roots, arg("screenshot"));
    screen.join().unwrap();
    assert_eq!(r.is_error, Some(false));
    assert_eq!(r.content.len(), 2, "説明と絵");
    drop(l);
    let _ = std::fs::remove_dir_all(&root);
}

/// `cell` を省けば操作の既定(設定の格子と荒削りの格子)に任せ、画面の無い同じ道で走らせた結果と
/// 同じになる。
#[test]
fn simulating_without_a_cell_uses_the_same_grid_as_the_screen() {
    let root = temp_root("sim-default");
    let mut s = Session::new(root.clone());
    s.sample(true, false).unwrap();
    let mcp = s.simulate(None, None).unwrap();
    let id = s
        .work
        .action_for_role(&jc_interchange::role::action::SIMULATE)
        .unwrap();
    let setup = s
        .work
        .doc()
        .data()
        .features
        .iter()
        .find(|f| f.type_id == "cam.setup@1")
        .map(|f| f.id);
    let jc_session::Ran::Report(r) = s.work.run(&id, setup, None, &Value::Null).unwrap() else {
        panic!("シミュレーションの結果は報告");
    };
    assert_eq!(mcp["summary"], r.info["summary"], "{mcp}");
    std::fs::remove_dir_all(&root).ok();
}

/// `.dxf`・`.svg` は、スケッチの書き出し(「スケッチを選んでから」で失敗する)でなく、ボディの
/// 書き出しの役割の操作から選ぶ。
#[test]
fn the_view_is_exported_to_dxf_and_svg() {
    let root = temp_root("export-view");
    let mut s = Session::new(root.clone());
    s.sample(false, false).unwrap();
    for ext in ["dxf", "svg"] {
        let r = s.export(&format!("a.{ext}"), None, Some(&json!("top")), None, false);
        assert!(r.is_ok(), "{ext}: {r:?}");
        let text = std::fs::read_to_string(root.join(format!("a.{ext}"))).unwrap();
        assert!(
            text.contains(if ext == "dxf" { "ENTITIES" } else { "<svg" }),
            "{text}"
        );
    }
    // 部品表(.csv)はボディの書き出しでない
    let r = s.export("a.csv", None, None, None, false);
    assert!(r.is_err(), "{r:?}");
    std::fs::remove_dir_all(&root).ok();
}

/// インチの文書では status の値も文書の単位(in)で出す。書く数だけの値は文書の単位で読むので、
/// 読んだ数をそのまま書き戻しても変わらない。
#[test]
fn values_read_in_an_inch_document_round_trip_through_set_value() {
    let root = temp_root("inch-round-trip");
    let mut s = Session::new(root.clone());
    s.sample(false, false).unwrap();
    let mut set = std::collections::BTreeMap::new();
    set.insert("units".to_string(), "in".to_string());
    s.properties(&set).unwrap();
    let st = s.status();
    assert_eq!(st["units"], "in", "{st}");
    let before = s.mass().unwrap();
    let values = st["values"].as_array().unwrap().clone();
    let mut lengths = 0;
    for v in &values {
        let Some(value) = v["value"].as_str() else {
            continue;
        };
        let Some((number, unit)) = value.split_once(' ') else {
            continue;
        };
        if unit != "in" && unit != "mm" {
            continue;
        }
        lengths += 1;
        assert_eq!(unit, "in", "長さはインチの文書なら in で読む: {v}");
        // 読んだ数だけを書き戻す(AI が単位を落としても値は変わらない)
        let r = s.set_value(
            v["feature"].as_u64().unwrap(),
            v["key"].as_str().unwrap(),
            number,
        );
        assert!(r.is_ok(), "{v}: {r:?}");
    }
    assert!(lengths > 0, "{st}");
    let after = s.mass().unwrap();
    let vol = |m: &Value| m[0]["volume"].as_f64().unwrap();
    assert!(
        (vol(&before) - vol(&after)).abs() < 1e-6 * vol(&before),
        "{before} → {after}"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// 文字でない中身(3MF・STL など)は、文字にすると壊れるので書き出し先 `out` を求める。
#[test]
fn a_binary_file_needs_an_out_path() {
    let root = temp_root("binary-out");
    let mut s = Session::new(root.clone());
    s.sample(false, false).unwrap();
    let r = s.run_action("io.export_3mf", None, None, &json!({}), None);
    let e = r.expect_err("3MF は文字にできない");
    assert!(e.message.contains("out"), "{e}");
    // 文字のファイル(G-code など)は今まで通り中身を返す
    let r = s
        .run_action(
            "io.export_view_svg",
            None,
            None,
            &json!({"view": "top"}),
            None,
        )
        .unwrap();
    assert!(r["contents"].as_str().unwrap().contains("<svg"), "{r}");
    std::fs::remove_dir_all(&root).ok();
}

/// 面に付けた外観の色は、3MF の書き出しでもその面の三角形に残る(スライサーで付け直さない)。
#[test]
fn a_face_appearance_is_kept_in_the_3mf() {
    let root = temp_root("3mf-face");
    let mut s = Session::new(root.clone());
    s.sample(false, false).unwrap();
    let topo = s.topology(None).unwrap();
    let body = topo["bodies"][0]["id"].as_u64().unwrap();
    let face = topo["bodies"][0]["faces"][0]["name"].clone();
    s.run_action(
        "render.appearance",
        None,
        None,
        &json!({"target": "face", "faces": [{"kind": "face", "feature": body, "face": face}], "apply": "赤"}),
        None,
    )
    .unwrap();
    let out = root.join("形.3mf");
    s.run_action("io.export_3mf", None, None, &json!({}), Some(&out))
        .unwrap();
    let z = std::fs::read(&out).unwrap();
    let model = jc_format::zip::read_written(&z, "3D/3dmodel.model").unwrap();
    let model = String::from_utf8(model).unwrap();
    assert!(model.contains("<base name=\"赤\""), "{model}");
    assert!(
        model.contains(" p1=\"1\"/>"),
        "面の三角形に赤が付いていない"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// 操作の引数は宣言(actions の args)で確かめる。知らない鍵を黙って捨てると、`tolerence` の
/// 打ち間違いで細かい網のつもりが既定の網になる。
#[test]
fn run_action_checks_the_arguments_against_the_declaration() {
    let root = temp_root("declared-args");
    let mut s = Session::new(root.clone());
    s.sample(false, false).unwrap();
    let list = s.actions(None);
    let stl = list
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "io.export_stl")
        .unwrap();
    let keys: Vec<&str> = stl["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["exclude", "tolerance", "ascii", "each"]);
    assert_eq!(stl["args"][1]["unit"]["kind"], "length");
    let e = s
        .run_action_at(
            "io.export_stl",
            None,
            None,
            &json!({"tolerence": 0.1}),
            Some("a.stl"),
        )
        .unwrap_err();
    assert!(e.message.contains("引数 tolerence はありません"), "{e}");
    assert_eq!(e.kind, jc_core::ProblemKind::Input);
    assert!(!root.join("a.stl").exists());
    // 誤りの答えは文と種類を分けて返す(相手が文の字で見分けずに済む。D10)
    let r = crate::server::problem(e);
    assert_eq!(r.is_error, Some(true));
    let sc = r.structured_content.expect("種類つき");
    assert_eq!(sc["kind"], "input");
    assert!(sc["message"].as_str().unwrap().contains("tolerence"));
    // 使えない操作(スケッチを選んでいない DXF の書き出し)は unavailable
    let e = s
        .run_action_at(
            "cad.sketch.export_dxf",
            None,
            None,
            &json!({}),
            Some("a.dxf"),
        )
        .unwrap_err();
    assert_eq!(e.kind, jc_core::ProblemKind::Unavailable, "{e}");
    let e = s
        .run_action_at(
            "io.export_stl",
            None,
            None,
            &json!({"tolerance": "fine"}),
            Some("a.stl"),
        )
        .unwrap_err();
    assert!(e.message.contains("tolerance"), "{e}");
    s.run_action_at(
        "io.export_stl",
        None,
        None,
        &json!({"tolerance": "0.05 mm"}),
        Some("a.stl"),
    )
    .unwrap();
    assert!(root.join("a.stl").exists());
    // 近道の export も、受け取らない鍵(STEP に format)を渡さない
    s.export("a.step", None, None, None, false).unwrap();
    std::fs::remove_dir_all(&root).ok();
}

/// MCP の app の説明は、画面の道具の一覧(jc_session::live::APP_TOOLS)を全部書く。
#[test]
fn the_app_tool_describes_every_screen_tool() {
    let schema = serde_json::to_value(schemars::schema_for!(crate::server::AppArg)).unwrap();
    let text = schema["properties"]["tool"]["description"]
        .as_str()
        .expect("tool の説明")
        .to_string();
    for tool in jc_session::live::APP_TOOLS {
        // 名前の前後が英数字でない所に書いてある(run が runner などの中に紛れない)
        let found = text.match_indices(tool).any(|(i, _)| {
            let before = text[..i].chars().next_back();
            let after = text[i + tool.len()..].chars().next();
            let edge = |c: Option<char>| c.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
            edge(before) && edge(after)
        });
        assert!(found, "app の説明に {tool} がありません: {text}");
    }
}

/// AI がスケッチを作る時、数でない位置(文字の "NaN"、範囲外の数)は読み込みで
/// 断り、文書に足さない。既にある機能の書き換えも同じに断る。スケッチの原点は面の角の名前で指す形で、数を受け取らない。
#[test]
fn a_sketch_with_a_non_numeric_position_is_refused() {
    let mut s = Session::new(temp_root("nan"));
    let before = s.status();
    for bad in [json!("NaN"), json!("inf"), json!(null)] {
        let r = s.add_feature(
            "cad.sketch@1",
            Some("点"),
            json!({"entities": [{"type": "point", "id": 1, "x": bad, "y": 8.0}]}),
        );
        assert!(r.is_err(), "{bad} を受け付けた: {r:?}");
    }
    // JSON の 1e999 は読む時点で誤り(無限大を作らない)
    assert!(serde_json::from_str::<Value>(r#"{"x": 1e999}"#).is_err());
    assert_eq!(s.status(), before, "断った物が文書に残った");
    // 既にある機能を書き換える時も同じに断り、元のデータのまま残す
    let ok = json!({"entities": [{"type": "point", "id": 1, "x": 1.0, "y": 8.0}]});
    s.add_feature("cad.sketch@1", Some("点"), ok).unwrap();
    let id = s.document().data().features.last().unwrap().id.0;
    let before = s.status();
    for bad in [json!("NaN"), json!(null)] {
        let r = s.set_feature(
            id,
            json!({"entities": [{"type": "point", "id": 1, "x": bad, "y": 8.0}]}),
        );
        assert!(r.is_err(), "{bad} で書き換えた: {r:?}");
    }
    assert_eq!(s.status(), before, "断った書き換えが文書に残った");
}

const IO_REFERENCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/入出力.md");

/// docs/入出力.md が今の登録と道具に合っている(古ければ `cargo xtask io` で書き直す)。
#[test]
fn io_reference_is_up_to_date() {
    let text = crate::reference::io_reference();
    let now = std::fs::read_to_string(IO_REFERENCE)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if now == text {
        // 合っている
    } else if std::env::var("JC_BLESS").is_ok_and(|v| v == "1") {
        std::fs::write(IO_REFERENCE, &text).unwrap();
    } else {
        let old: std::collections::BTreeSet<&str> = now.lines().collect();
        let new: std::collections::BTreeSet<&str> = text.lines().collect();
        let gone: Vec<&&str> = old.difference(&new).take(20).collect();
        let added: Vec<&&str> = new.difference(&old).take(20).collect();
        panic!(
            "docs/入出力.md が今の登録と違う。cargo xtask io で書き直す\n\
             無くなった行: {gone:?}\n増えた行: {added:?}"
        );
    }
}

/// レンダリングのシーンの設定は文書ごと。変えると
/// 今の文書の区画 `render.scene` に入り(取り消せる)、まだ変えていない文書と新しい文書は前回使った設定。
#[test]
fn the_render_scene_is_kept_per_document() {
    let root = temp_root("scene");
    let mut s = Session::new(root.clone());
    let width = |v: &Value| {
        v.as_array()
            .unwrap()
            .iter()
            .find(|g| g["id"] == "render.scene")
            .map(|g| g["value"]["width"].clone())
            .expect("シーンの設定")
    };
    s.sample(false, false).unwrap();
    let r = s
        .set_settings("render.scene", &json!({"width": 800}))
        .unwrap();
    assert_eq!(width(&r), 800);
    assert_eq!(
        s.history()["done"].as_array().unwrap().last().unwrap(),
        "レンダリングの設定を変える"
    );
    s.save(Some("a.json"), true).unwrap();
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("a.json")).unwrap()).unwrap();
    assert_eq!(saved["sections"]["render.scene"]["data"]["width"], 800);
    // 新しい文書は前回使った設定。そこで変えても、前の文書の設定は変わらない
    s.sample(false, false).unwrap();
    assert_eq!(width(&s.settings()), 800);
    s.set_settings("render.scene", &json!({"width": 640}))
        .unwrap();
    assert_eq!(width(&s.settings()), 640);
    // 取り消すと文書の区画が外れる(見えるのは前回使った設定)
    s.undo().unwrap();
    assert_eq!(s.history()["undone"], json!(["レンダリングの設定を変える"]));
    assert_eq!(width(&s.settings()), 640);
    // 前の文書を開くと、その文書の設定
    s.open("a.json").unwrap();
    assert_eq!(width(&s.settings()), 800);
    // 正しくない値は断り、文書も PC 全体の値も変えない
    assert!(
        s.set_settings("render.scene", &json!({"width": 1}))
            .is_err()
    );
    assert_eq!(width(&s.settings()), 800);
    assert!(s.history()["done"].as_array().unwrap().is_empty());
}

#[test]
fn a_parameter_gets_its_comment_and_unit_from_the_ai() {
    let root = temp_root("param-info");
    let mut s = Session::new(root);
    s.set_param_info("pitch", "3", Some("穴の間隔"), Some("length"))
        .unwrap();
    let p = |s: &Session| s.work.doc().data().param_by_name("pitch").cloned().unwrap();
    assert_eq!(p(&s).comment, "穴の間隔");
    assert_eq!(p(&s).unit, jc_core::ParamUnit::Length);
    // 式とコメントを変えるのは 1 回で取り消せる。省いた単位は今のまま
    let depth = s.work.doc().undo_depth();
    s.set_param_info("pitch", "4", Some("間隔"), None).unwrap();
    assert_eq!(s.work.doc().undo_depth(), depth + 1);
    assert_eq!((p(&s).expr.as_str(), p(&s).comment.as_str()), ("4", "間隔"));
    assert_eq!(p(&s).unit, jc_core::ParamUnit::Length);
    // 知らない単位は断り、文書を変えない
    assert!(s.set_param_info("pitch", "5", None, Some("feet")).is_err());
    assert_eq!(p(&s).expr, "4");
}

#[test]
fn the_ai_moves_features_in_the_timeline() {
    let mut s = Session::new(temp_root("move"));
    let pt = json!({"entities": [{"type": "point", "id": 1, "x": 1.0, "y": 8.0}]});
    for name in ["A", "B", "C"] {
        s.add_feature("cad.sketch@1", Some(name), pt.clone())
            .unwrap();
    }
    let order = |s: &Session| -> Vec<String> {
        s.document()
            .data()
            .features
            .iter()
            .map(|f| f.name.clone())
            .collect()
    };
    let id = |s: &Session, n: &str| {
        s.document()
            .data()
            .features
            .iter()
            .find(|f| f.name == n)
            .unwrap()
            .id
            .0
    };
    let (a, b, c) = (id(&s, "A"), id(&s, "B"), id(&s, "C"));
    // B と C をまとめて A の前へ。1 回で取り消せる
    s.move_features(&[c, b], a, false).unwrap();
    assert_eq!(order(&s), ["B", "C", "A"]);
    s.undo().unwrap();
    assert_eq!(order(&s), ["A", "B", "C"]);
    // バッチの手からも同じに動かせる
    s.batch(vec![
        serde_json::from_value(
            json!({"tool": "move_features", "args": {"ids": [a], "target": c, "after": true}}),
        )
        .unwrap(),
    ])
    .unwrap();
    assert_eq!(order(&s), ["B", "C", "A"]);
}

#[test]
fn the_ai_saves_and_uses_named_views() {
    let mut s = Session::new(temp_root("views"));
    s.sample(true, false).unwrap();
    let n0 = s.named_views()["named_views"].as_array().unwrap().len();
    let a: crate::server::SaveViewArg = serde_json::from_value(json!({
        "name": "斜め", "target": [1.0, 2.0, 3.0], "yaw": -60.0, "pitch": 30.0, "roll": 15.0,
        "height": 80.0
    }))
    .unwrap();
    let v = s.save_named_view(&a).unwrap();
    let id = v["id"].as_u64().unwrap();
    assert_eq!(v["name"], "斜め");
    assert_eq!(v["roll"], 15.0, "傾きも残す");
    // 同じ名前は番号を足す。向きが正しくない視点は断る
    let again = s.save_named_view(&a).unwrap();
    assert_eq!(again["name"], "斜め (2)");
    let bad: crate::server::SaveViewArg = serde_json::from_value(
        json!({"target": [0.0, 0.0, 0.0], "yaw": 0.0, "pitch": 120.0, "height": 10.0}),
    )
    .unwrap();
    assert!(s.save_named_view(&bad).is_err());
    let list = s.rename_named_view(id, "前の斜め").unwrap();
    let named = |l: &Value| -> Vec<String> {
        l["named_views"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert!(named(&list).contains(&"前の斜め".to_string()), "{list}");
    // 名前を付けた視点で描ける。無い番号は断る
    let shot = |view: u64| -> crate::server::ScreenshotArg {
        serde_json::from_value(json!({"named_view": view, "width": 32, "height": 24})).unwrap()
    };
    assert!(s.screenshot(&shot(id)).is_ok());
    assert!(s.screenshot(&shot(999)).is_err());
    let list = s.remove_named_view(id).unwrap();
    assert_eq!(list["named_views"].as_array().unwrap().len(), n0 + 1);
    s.undo().unwrap();
    assert_eq!(
        s.named_views()["named_views"].as_array().unwrap().len(),
        n0 + 2
    );
}

#[test]
fn the_ai_copies_features_and_links_the_copies_together() {
    let mut s = Session::new(temp_root("paste"));
    s.sample(true, false).unwrap();
    let feats = s.status()["features"].as_array().unwrap().clone();
    let pocket = feats
        .iter()
        .find(|f| f["type"] == "solid.extrude@1" && f["name"] == "ポケット")
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    // ポケットが削るボディを作った機能(ポケットのデータの中の、前の機能を指す数)
    let data = s
        .document()
        .data()
        .feature(jc_core::FeatureId(pocket))
        .unwrap()
        .data
        .clone();
    let base = feats
        .iter()
        .map(|f| f["id"].as_u64().unwrap())
        .find(|id| *id != pocket && data.as_object().unwrap().values().any(|v| v == &json!(id)))
        .unwrap_or_else(|| panic!("ボディの欄が無い: {data}"));
    let r = s.paste_features(&[base, pocket]).unwrap();
    let pasted: Vec<u64> = serde_json::from_value(r["pasted"].clone()).unwrap();
    assert_eq!(pasted.len(), 2, "{r}");
    // 貼ったポケットは、元の形でなく貼った写しを削る
    let copy = s
        .document()
        .data()
        .feature(jc_core::FeatureId(pasted[1]))
        .unwrap()
        .data
        .clone();
    let values: Vec<&Value> = copy.as_object().unwrap().values().collect();
    assert!(values.contains(&&json!(pasted[0])), "{copy}");
    assert!(!values.contains(&&json!(base)), "{copy}");
    // スケッチ(写せない機能)を含めると断る。無い番号も断る
    let sketch = feats.iter().find(|f| f["type"] == "cad.sketch@1").unwrap()["id"]
        .as_u64()
        .unwrap();
    assert!(s.paste_features(&[sketch]).is_err());
    assert!(s.paste_features(&[9999]).is_err());
    s.undo().unwrap();
    assert_eq!(
        s.status()["features"].as_array().unwrap().len(),
        feats.len()
    );
}

/// 部品の木の並べ替え・ボディを移す・複製を AI からも(画面と同じ組み立て)。
#[test]
fn the_ai_moves_and_duplicates_components() {
    let mut s = Session::new(temp_root("components"));
    s.sample(true, false).unwrap();
    let feats = s.status()["features"].as_array().unwrap().clone();
    let pocket = feats
        .iter()
        .find(|f| f["type"] == "solid.extrude@1" && f["name"] == "ポケット")
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let args = json!({"name": "台", "selection": [pocket]});
    s.run_action("asm.new_component", None, None, &args, None)
        .unwrap();
    let args = json!({"name": "空", "selection": []});
    s.run_action("asm.new_component", None, None, &args, None)
        .unwrap();
    let ids = |s: &Session| -> Vec<(u64, Option<u64>)> {
        s.document()
            .data()
            .components
            .iter()
            .map(|c| (c.id, c.parent))
            .collect()
    };
    let [(a, None), (b, None)] = ids(&s)[..] else {
        panic!("{:?}", ids(&s));
    };
    // 空を台の前へ、次に台の中へ。自分の中へは移せない
    let call = |before: Option<u64>, into: Option<u64>| crate::server::MoveComponentArg {
        id: b,
        into,
        before,
        after: None,
    };
    s.move_component(&call(Some(a), None)).unwrap();
    assert_eq!(ids(&s), vec![(b, None), (a, None)]);
    s.move_component(&call(None, Some(a))).unwrap();
    assert_eq!(ids(&s), vec![(a, None), (b, Some(a))]);
    let back = crate::server::MoveComponentArg {
        id: a,
        into: Some(b),
        before: None,
        after: None,
    };
    assert!(s.move_component(&back).is_err());
    // ボディを空の部品へ移し、外す
    s.move_bodies(&[pocket], Some(b)).unwrap();
    assert_eq!(s.document().data().component(b).unwrap().bodies.len(), 1);
    assert!(s.document().data().component(a).unwrap().bodies.is_empty());
    s.move_bodies(&[pocket], None).unwrap();
    assert!(s.document().data().component(b).unwrap().bodies.is_empty());
    s.move_bodies(&[pocket], Some(a)).unwrap();
    // 台を中の部品ごと複製: 元のすぐ後ろに並び、写しのボディが入る
    let r = s.duplicate_component(a).unwrap();
    let top = r["component"].as_u64().unwrap();
    let data = s.document().data();
    let made = data.component(top).unwrap();
    assert_eq!(made.name, "台 (2)");
    assert_eq!(made.bodies.len(), 1);
    assert_ne!(made.bodies[0].0, pocket);
    assert_eq!(data.child_components(Some(top)).len(), 1);
    let tops: Vec<u64> = data.child_components(None).iter().map(|c| c.id).collect();
    assert_eq!(tops, vec![a, top]);
    assert!(s.duplicate_component(9999).is_err());
}

/// 作業する部品(アクティブな部品)を AI からも決められ、その後に作ったボディと部品が入る
/// (画面のブラウザの丸と同じ。作った変更と 1 回の取り消しで戻る)。
#[test]
fn the_ai_activates_a_component_and_new_bodies_go_into_it() {
    let mut s = Session::new(temp_root("active-component"));
    s.sample(true, false).unwrap();
    let feats = s.status()["features"].as_array().unwrap().clone();
    let pocket = feats
        .iter()
        .find(|f| f["type"] == "solid.extrude@1" && f["name"] == "ポケット")
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let args = json!({"name": "空", "selection": []});
    s.run_action("asm.new_component", None, None, &args, None)
        .unwrap();
    let c = s.document().data().components[0].id;
    assert!(s.activate_component(Some(9999)).is_err());
    let r = s.activate_component(Some(c)).unwrap();
    assert_eq!(r["active_component"], json!(c));
    let undo_before = s.document().undo_depth();
    let copy = json!({"body": pocket, "copy": true, "z": "50 mm"});
    let made = s.add_feature("solid.move@1", None, copy).unwrap()["added"]
        .as_u64()
        .unwrap();
    let data = s.document().data();
    assert_eq!(
        data.component_of(jc_core::FeatureId(made)).map(|k| k.id),
        Some(c)
    );
    assert_eq!(
        s.document().undo_depth(),
        undo_before + 1,
        "同じ取り消し単位"
    );
    // 直下に作った部品も中へ
    let args = json!({"name": "中", "selection": []});
    s.run_action("asm.new_component", None, None, &args, None)
        .unwrap();
    let inner = s
        .document()
        .data()
        .components
        .iter()
        .find(|k| k.name == "中")
        .unwrap();
    assert_eq!(inner.parent, Some(c));
    // 直下へ戻すと入らない
    let r = s.activate_component(None).unwrap();
    assert!(r.get("active_component").is_none());
    let copy = json!({"body": pocket, "copy": true, "z": "90 mm"});
    let other = s.add_feature("solid.move@1", None, copy).unwrap()["added"]
        .as_u64()
        .unwrap();
    assert!(
        s.document()
            .data()
            .component_of(jc_core::FeatureId(other))
            .is_none()
    );
}

/// ロールバックの位置は画面・CLI と同じ操作で動かし、範囲の外は何も変えずに入力の誤りで止まる
/// (10-24 h6・10-20 p9)。
#[test]
fn roll_back_moves_the_timeline_mark_and_refuses_positions_out_of_range() {
    let root = temp_root("roll-back");
    let mut s = Session::new(root);
    s.sample(false, false).unwrap();
    let rolled = |st: &Value| {
        st["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["status"] == "rolled_back")
            .count()
    };
    let n = s.status()["features"].as_array().unwrap().len();
    assert!(n >= 2, "見本に機能が要る");
    let st = s.roll_back(Some(1)).unwrap();
    assert_eq!(rolled(&st), n - 1, "{st}");
    for bad in [-1, n as i64 + 1] {
        let e = s.roll_back(Some(bad)).unwrap_err();
        assert_eq!(e.kind, jc_core::ProblemKind::Input, "{e}");
        assert!(e.message.contains("範囲の外"), "{e}");
    }
    assert_eq!(rolled(&s.status()), n - 1, "断った時は位置を変えない");
    // 機能の数と省いた時は最後まで進める
    let st = s.roll_back(Some(n as i64)).unwrap();
    assert_eq!(rolled(&st), 0, "{st}");
    s.roll_back(Some(0)).unwrap();
    assert_eq!(rolled(&s.roll_back(None).unwrap()), 0);
    // 1 回の取り消しで前の位置に戻る
    assert_eq!(rolled(&s.undo().unwrap()), n);
}

/// 打ち間違えた操作の ID には、近い ID を添えて断る(10-24 m1)。
#[test]
fn a_mistyped_action_id_suggests_the_close_ones() {
    let root = temp_root("near-action");
    let mut s = Session::new(root);
    s.sample(false, false).unwrap();
    let e = s
        .run_action_at("io.exprot_stl", None, None, &json!({}), Some("a.stl"))
        .unwrap_err();
    assert!(e.message.contains("近い名前: io.export_stl"), "{e}");
}
