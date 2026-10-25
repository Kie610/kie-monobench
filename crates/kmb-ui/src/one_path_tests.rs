//! 道が 1 本: 同じ操作と引数を、画面(`Workspace::run_action_args`)と
//! CLI・MCP(どちらも `kmb_session::Headless::run` を呼ぶ。jc `run` と MCP の `run_action`)で
//! 流すと、同じ文書と同じ計算の結果になり、断る操作も同じに断る。

use crate::Workspace;
use kmb_core::FeatureId;
use kmb_session::{Headless, Ran};
use serde_json::{Value, json};

/// 比べる中身: 文書のデータと、機能ごとの計算の結果(状態・理由)。
fn snapshot(h: &Headless) -> (Value, Value) {
    (
        serde_json::to_value(h.doc().data()).unwrap(),
        h.status()["features"].clone(),
    )
}

fn id_of(h: &Headless, name: &str) -> FeatureId {
    h.doc()
        .data()
        .features
        .iter()
        .find(|f| f.name == name)
        .map(|f| f.id)
        .unwrap_or_else(|| panic!("{name} が無い"))
}

#[test]
fn the_screen_and_the_cli_and_mcp_make_the_same_document_from_the_same_steps() {
    let mut ws = Workspace::new(kmb_samples::reference_document(), None, kmb_bundle::modules());
    ws.wait();
    let mut cli =
        Headless::with_document(kmb_bundle::modules(), kmb_samples::reference_document(), None);
    assert_eq!(snapshot(&ws.cur.work), snapshot(&cli), "始めから違う");
    let plate = id_of(&cli, "板");
    let boss = id_of(&cli, "ボス");

    // (操作, 選んでいる機能, 引数, 通るか)
    let steps: Vec<(&str, Option<FeatureId>, Value, bool)> = vec![
        ("cad.add.sketch", None, json!({"plane": "yz"}), true),
        // 選んだ機能が要る操作(面の参照を引数で渡す)
        (
            kmb_cad_extrude::EXTRUDE_TO_FACE,
            Some(boss),
            json!({"face": format!("f{}:start", plate.0)}),
            true,
        ),
        // 宣言に無い鍵は誤り(画面でも同じに断り、文書を変えない)
        (
            "cad.add.sketch",
            None,
            json!({"plane": "xy", "plaen": "yz"}),
            false,
        ),
        // 今使えない操作(スケッチを選んでいない)は画面でも断る
        ("cad.sketch.export_dxf", None, Value::Null, false),
    ];
    for (i, (id, selected, args, ok)) in steps.into_iter().enumerate() {
        match selected {
            Some(s) => ws.select(s),
            None => ws.clear_selection(),
        }
        ws.win.message = None;
        ws.run_action_args(id, args.clone());
        ws.wait();
        let screen_refused = ws.win.message.clone();
        let r = cli.run(id, selected, None, &args);
        match (&r, ok) {
            (Ok(Ran::Changed(_)), true) => {
                assert_eq!(screen_refused, None, "手 {i} {id}: 画面だけ断った")
            }
            (Err(p), false) => {
                let m = screen_refused
                    .unwrap_or_else(|| panic!("手 {i} {id}: CLI は断ったが画面は通した: {p}"));
                assert!(m.contains(&p.message), "手 {i} {id}: 画面 {m} / CLI {p}");
            }
            _ => panic!("手 {i} {id}: 思った結果でない: {:?}", r.map(|_| ())),
        }
        assert_eq!(
            snapshot(&ws.cur.work),
            snapshot(&cli),
            "手 {i} {id} の後の文書が違う"
        );
    }
}
