use super::*;
use kmb_cam_tool::ToolLibrary;
use kmb_toolpath::{ArcDir, Move, Toolpath};

/// `plugins/post-example` を WASM のコンポーネントにしたもの(作り方はその Cargo.toml)。
const EXAMPLE: &[u8] = include_bytes!("../../../plugins/post-example.wasm");

fn program() -> Program {
    let lib = ToolLibrary::starter();
    let tp = Toolpath {
        name: "らせん".into(),
        tool: lib.tools[1].clone(),
        rpm: 12000.0,
        safe_z: 5.0,
        retract_z: 1.0,
        edge_break: 0.0,
        dwells: vec![],
        spans: vec![],
        links: Default::default(),
        moves: vec![
            Move::Rapid {
                to: [12.0, 10.0, 5.0],
            },
            Move::Rapid {
                to: [12.0, 10.0, 1.0],
            },
            Move::Linear {
                to: [12.0, 10.0, 0.0],
                feed: 150.0,
            },
            Move::Arc {
                to: [8.0, 10.0, -0.5],
                center: [10.0, 10.0],
                dir: ArcDir::Ccw,
                feed: 400.0,
            },
            Move::Rapid {
                to: [8.0, 10.0, 5.0],
            },
        ],
    };
    Program::build("見本", 1, 20.0, &[tp])
}

// 見本のプラグインを 1 回だけ読み込んで、次をまとめて確かめる:
// - 見本のプラグインは G-code を書き、何度でも呼べる
// - ホストとプラグインの両方が検査する
// - 後から足した命令は、古いプラグインには知らせのコメントにするか断る
// - 長く走るプラグインは計算量の上限で止まり、上限を戻せばまた動く
#[test]
fn the_example_plugin_writes_gcode_checks_lowers_new_commands_and_stops_when_too_long() {
    let mut post = WasmPost::from_bytes(EXAMPLE).unwrap();
    assert_eq!(post.id(), "example-grbl");
    assert!(post.describe().contains("Grbl"));
    let m = kmb_post_carvera_air::profile();
    let g = post.emit(&program(), &m).unwrap();
    assert!(
        g.starts_with("(example-grbl / Carvera Air)\nG21 G90 G17 G54\n"),
        "{g}"
    );
    assert!(g.contains("M6 T2\n"), "{g}");
    assert!(g.contains("G3 X8 Y10 Z-0.5 I-2 J0 F400\n"), "{g}");
    assert!(g.ends_with("M5\nM30\n"), "{g}");
    // 同じプラグインを何度でも呼べる
    assert_eq!(post.emit(&program(), &m).unwrap(), g);

    // --- ホストとプラグインの両方が検査する
    let mut p = program();
    p.wcs = 7;
    let e = post.emit(&p, &m).unwrap_err().to_string();
    assert!(e.contains("G54〜G59"), "プラグインの検査: {e}");
    let mut p = program();
    p.ops.push(kmb_toolpath::Op::Spindle { rpm: 50000.0 });
    let e = post.emit(&p, &m).unwrap_err().to_string();
    assert!(e.contains("回転数"), "ホストの検査: {e}");

    // --- 後から足した命令(手動 NC)は、作り済みのプラグインに知らない種類として渡さない。
    // 知らせはコメントにして渡し、停止は黙って落とさずに誤りにする
    let lib = ToolLibrary::starter();
    let paths = two_slots(&lib);
    let msg = Program::build_with(
        "見本",
        1,
        20.0,
        &paths,
        &[(
            1,
            kmb_toolpath::Manual::Message {
                text: "切りくずを見る".into(),
            },
        )],
    );
    let g = post.emit(&msg, &m).unwrap();
    assert!(g.contains("(知らせ: 切りくずを見る)\n"), "{g}");
    let stop = Program::build_with(
        "見本",
        1,
        20.0,
        &paths,
        &[(
            1,
            kmb_toolpath::Manual::Stop {
                optional: false,
                text: String::new(),
            },
        )],
    );
    let e = post.emit(&stop, &m).unwrap_err().to_string();
    assert!(e.contains("停止(手動 NC)の命令を渡せません"), "{e}");
    // JSON の形: 停止とメッセージはコメントになる
    let mut v = serde_json::to_value(&stop).unwrap();
    ops_for_plugins(&mut v);
    assert!(
        v["ops"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["op"] != "stop" && o["op"] != "message")
    );

    // --- 長く走るプラグインは止める(計算量の上限を変えるので最後の段)
    post.set_fuel_per_call(10_000);
    let e = post
        .emit(&program(), &kmb_post_carvera_air::profile())
        .unwrap_err()
        .to_string();
    assert!(e.contains("計算量の上限"), "{e}");
    // 上限を戻せば、同じプラグインでまた動く
    post.set_fuel_per_call(FUEL_PER_CALL);
    assert!(
        post.emit(&program(), &kmb_post_carvera_air::profile())
            .is_ok()
    );
}

#[test]
fn newer_tool_kinds_are_passed_as_the_kinds_plugins_know() {
    let mut lib = ToolLibrary::starter();
    lib.tools[0].kind = kmb_cam_tool::ToolKind::BullNose;
    lib.tools[0].corner_radius = Some(1.0);
    lib.tools[3].kind = kmb_cam_tool::ToolKind::TaperBall;
    lib.tools[3].tip_diameter = Some(1.0);
    let mut v = serde_json::to_value(&lib).unwrap();
    tools_for_plugins(&mut v);
    assert_eq!(v["tools"][0]["kind"], "flat_end");
    assert!(v["tools"][0].get("corner_radius").is_none());
    assert_eq!(v["tools"][3]["kind"], "ball_end");
}

#[test]
fn broken_files_are_refused() {
    assert!(WasmPost::from_bytes(b"not wasm").is_err());
    assert!(WasmPost::from_file(Path::new("/nonexistent/x.wasm")).is_err());
}

fn two_slots(lib: &ToolLibrary) -> Vec<Toolpath> {
    let one = |x: f64| Toolpath {
        name: format!("溝 {x}"),
        tool: lib.tools[1].clone(),
        rpm: 12000.0,
        safe_z: 5.0,
        retract_z: 1.0,
        edge_break: 0.0,
        dwells: vec![],
        spans: vec![],
        links: Default::default(),
        moves: vec![
            Move::Rapid { to: [x, 10.0, 5.0] },
            Move::Linear {
                to: [x, 10.0, -0.5],
                feed: 150.0,
            },
            Move::Rapid { to: [x, 10.0, 5.0] },
        ],
    };
    vec![one(10.0), one(20.0)]
}

/// Cargo.lock の、ワークスペースの中の crate(`source` の無い物)ごとの依存の並び。
fn path_packages(lock: &str) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut out = std::collections::BTreeMap::new();
    for block in lock.split("[[package]]").skip(1) {
        if block.contains("\nsource = ") {
            continue;
        }
        let name = block
            .lines()
            .find_map(|l| l.strip_prefix("name = "))
            .map(|n| n.trim_matches('"').to_string())
            .unwrap_or_default();
        let deps = block
            .split_once("dependencies = [")
            .map(|(_, rest)| {
                rest.split(']')
                    .next()
                    .unwrap_or_default()
                    .lines()
                    // 版が 2 つある crate は本体の方だけ「名前 版」と書くので、名前だけで比べる
                    .map(|l| {
                        let l = l.trim().trim_end_matches(',').trim_matches('"');
                        l.split(' ').next().unwrap_or_default().to_string()
                    })
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        out.insert(name, deps);
    }
    out
}

/// Cargo.toml の `[dev-dependencies]` の名前(dev だけの依存。普通の依存にもある物は除く)。
fn dev_dependencies(toml: &str) -> Vec<String> {
    let mut section = "";
    let (mut dev, mut normal) = (Vec::new(), Vec::new());
    for line in toml.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line;
            continue;
        }
        let Some((name, _)) = line.split_once('=') else {
            continue;
        };
        // `proptest.workspace = true` の形も名前だけに
        let name = name
            .trim()
            .split('.')
            .next()
            .unwrap_or_default()
            .to_string();
        match section {
            "[dev-dependencies]" => dev.push(name),
            "[dependencies]" => normal.push(name),
            _ => {}
        }
    }
    dev.retain(|d| !normal.contains(d));
    dev
}

/// 見本のプラグインは本体と別のワークスペースなので、本体の crate の依存を変えても、その
/// Cargo.lock は誰も直さない。本体の crate に足した依存が入らないと `cargo build --locked` で
/// 作れなくなるので、本体の crate の依存の並びが
/// 本体の Cargo.lock と同じかを見る(違えば、本体の Cargo.lock を写して
/// `cargo metadata --manifest-path plugins/post-example/Cargo.toml` で絞り直す)。
#[test]
fn the_example_plugin_lock_follows_the_workspace() {
    let root = path_packages(include_str!("../../../Cargo.lock"));
    let plugin = path_packages(include_str!("../../../plugins/post-example/Cargo.lock"));
    let mut checked = 0;
    for (name, deps) in &plugin {
        if name == "kmb-post-example" {
            continue;
        }
        let want = root
            .get(name)
            .unwrap_or_else(|| panic!("{name} は本体のワークスペースに無い"));
        // 外のワークスペースから引いた crate は dev の依存を解かないので、本体の並びから dev の
        // 依存を除いて比べる
        let toml = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../{name}/Cargo.toml")),
        )
        .unwrap_or_else(|e| panic!("crates/{name}/Cargo.toml: {e}"));
        let dev = dev_dependencies(&toml);
        let want: Vec<String> = want.iter().filter(|d| !dev.contains(d)).cloned().collect();
        assert_eq!(
            deps, &want,
            "plugins/post-example/Cargo.lock の {name} の依存が本体と違う"
        );
        checked += 1;
    }
    assert!(checked >= 2, "kmb-post・kmb-toolpath を見ている: {plugin:?}");
}

/// 見本のプラグインは画面の部品を組まない。揃えた依存(kmb-workspace-hack)は画面の部品を抱えるので、
/// プラグイン作りに使う crate は `.config/hakari.toml` の `final-excludes` で揃えた依存から外してある。
/// 外し忘れた crate が依存に入ると、プラグインが WASM でも egui などを組むようになる。
#[test]
fn the_example_plugin_builds_without_screen_parts() {
    let lock = include_str!("../../../plugins/post-example/Cargo.lock");
    let names: Vec<&str> = lock
        .lines()
        .filter_map(|l| l.strip_prefix("name = "))
        .map(|n| n.trim_matches('"'))
        .collect();
    assert!(names.contains(&"kmb-post"), "Cargo.lock を読めている: {names:?}");
    let bad: Vec<&&str> = names
        .iter()
        .filter(|n| {
            **n == "kmb-workspace-hack"
                || ["egui", "eframe", "epaint", "winit", "wgpu"]
                    .iter()
                    .any(|p| n.starts_with(p))
        })
        .collect();
    assert!(
        bad.is_empty(),
        "plugins/post-example/Cargo.lock に {bad:?} がある(プラグインの依存の crate を \
         .config/hakari.toml の final-excludes に足す)"
    );
}
