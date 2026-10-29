//! 解析の研究を画面で解く試験(人の操作の順): 研究を選ぶと「未実行」で、設定欄の「実行」を押すと
//! 計算中の様子が出て「済み」になり、結果の色分けが出る。条件を変えると「古い」になって、
//! ボタンが「実行(解き直す)」になる。流れは時刻の帯でコマを送れる。

use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_core::{FeatureEntry, Op, RunTone};
use kmb_samples::reference_document;
use serde_json::json;

fn harness(ws: Workspace) -> Harness<'static, Workspace> {
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws)
}

/// 見本の部品に研究 `type_id` を足した文書と、研究の番号。
fn with_study(type_id: &str, data: serde_json::Value) -> (Workspace, FeatureId) {
    let mut doc = reference_document();
    let id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "研究",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id,
                type_id: type_id.into(),
                name: "研究 1".into(),
                data,
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let mut ws = Workspace::new(doc, None, kmb_bundle::modules());
    ws.switch_space(crate::layout::Space::Simulation);
    ws.select(id);
    (ws, id)
}

/// 計算が終わるまで画面を回す(長くても `secs` 秒)。
fn wait_done(h: &mut Harness<'static, Workspace>, id: FeatureId, secs: u64) {
    let t0 = std::time::Instant::now();
    loop {
        h.step();
        let st = h.state().run_state(id).expect("研究の様子");
        if st.tone != RunTone::Busy {
            break;
        }
        assert!(t0.elapsed().as_secs() < secs, "終わらない: {}", st.badge);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    h.run();
}

#[test]
fn a_study_runs_from_its_panel_and_turns_stale_when_its_conditions_change() {
    let (ws, id) = with_study(
        "fea.static@1",
        json!({
            "material": "アルミ",
            "fixed": {"kind": "face", "feature": 8, "face": "f8:side:e6"},
            "loads": [{"faces": {"kind": "face", "feature": 8, "face": "f8:end"}, "kind": "force", "value": "100", "dir": "-z"}],
            "size": "4 mm",
        }),
    );
    let mut h = harness(ws);
    h.run();
    // 選んだだけでは解かない(色分けもない)
    let st = h.state().run_state(id).expect("研究の様子");
    assert_eq!(st.tone, RunTone::Idle, "{}", st.badge);
    assert_eq!(st.badge, "未実行");
    assert!(h.state().overlay().shape.is_none());
    // 開いた設定欄の「実行」を押す
    h.state_mut().open_edit(id);
    h.run();
    h.get_by_label("▶  実行").click();
    h.step();
    wait_done(&mut h, id, 300);
    let st = h.state().run_state(id).unwrap();
    assert_eq!(st.tone, RunTone::Done, "{} {}", st.badge, st.detail);
    let shape = h.state().overlay().shape.expect("結果の色分け");
    assert!(shape.label.starts_with("ミーゼス応力"), "{}", shape.label);
    assert!(shape.badge.starts_with("2 次の四面体"), "{}", shape.badge);
    // 条件を変えると「古い」。ボタンは「実行(解き直す)」になり、見出しに「古い」が出る
    let mut data = h
        .state()
        .cur
        .work
        .doc()
        .data()
        .feature(id)
        .unwrap()
        .data
        .clone();
    data["material"] = json!("鉄");
    h.state_mut().apply(Transaction::new(
        "材料",
        vec![Op::SetFeatureData { id, data }],
    ));
    h.run();
    let st = h.state().run_state(id).unwrap();
    assert_eq!(st.tone, RunTone::Stale, "{}", st.badge);
    assert!(h.state().overlay().shape.unwrap().label.contains("古い"));
    h.state_mut().open_edit(id);
    h.run();
    h.get_by_label("▶  実行(解き直す)").click();
    h.step();
    wait_done(&mut h, id, 300);
    assert_eq!(h.state().run_state(id).unwrap().tone, RunTone::Done);
}

#[test]
fn a_flow_study_steps_through_its_frames_on_the_time_bar() {
    // 見本の部品のまわりの 2D の流れ(粗い格子・コマ 6 枚で短く)
    let (ws, id) = with_study(
        "flow.section@1",
        json!({
            "bodies": [{"kind": "body", "feature": 8}],
            "plane": "xy", "dir": "+x", "speed": "1", "fluid": "air",
            "size": "3 mm", "end": "0.05", "frames": "6",
        }),
    );
    let mut h = harness(ws);
    h.state_mut().open_edit(id);
    h.run();
    h.get_by_label("▶  実行").click();
    h.step();
    wait_done(&mut h, id, 300);
    let st = h.state().run_state(id).unwrap();
    assert_eq!(st.tone, RunTone::Done, "{} {}", st.badge, st.detail);
    // 時刻の帯: 最後のコマから始まり、ボタンで最初のコマへ・次のコマへ送れる
    let bar = h.state().frame_bar().expect("時刻の帯");
    assert_eq!(bar.labels.len(), 6, "{:?}", bar.labels);
    assert_eq!(bar.at, 5);
    h.get_by_label("最初のコマ").click();
    h.run();
    assert_eq!(h.state().frame_bar().unwrap().at, 0);
    h.get_by_label("次のコマ").click();
    h.run();
    let bar = h.state().frame_bar().unwrap();
    assert_eq!(bar.at, 1);
    let shape = h.state().overlay().shape.expect("流れの色分け");
    assert!(shape.label.contains(&bar.labels[1]), "{}", shape.label);
    // 再生すると最後のコマまで進む
    h.get_by_label("再生").click();
    let t0 = std::time::Instant::now();
    while h.state().frame_bar().unwrap().at < 5 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(t0.elapsed().as_secs() < 20, "再生が進まない");
    }
}

/// 確かめること: 実際の固有振動の研究を画面の道具で解き、再生と停止で揺れの形が変わる。
#[test]
fn a_modal_study_plays_and_pauses_through_the_connected_ui_tools() {
    let (mut ws, id) = with_study(
        "fea.modal@1",
        json!({
            "material": "アルミ",
            "fixed": {"kind": "face", "feature": 8, "face": "f8:side:e6"},
            "size": "6 mm", "modes": "2",
        }),
    );
    let dir = std::env::temp_dir().join(format!(
        "modal-ui-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    ws = ws.with_data_dir(dir.clone());
    // データフォルダの設定を読んだ後に、試す作業空間を選ぶ。
    ws.switch_space(crate::layout::Space::Simulation);
    ws.layout_mut().ai_live = true;
    ws.open_edit(id);
    let mut h = harness(ws);
    let clock = std::cell::Cell::new(100.0);
    let ask = |h: &mut Harness<'static, Workspace>, tool: &str, args: serde_json::Value| {
        let data = dir.clone();
        let tool = tool.to_string();
        let request = std::thread::spawn(move || kmb_session::live::call(&data, &tool, args));
        let start = std::time::Instant::now();
        while !request.is_finished() {
            h.input_mut().time = Some(clock.get());
            h.step();
            assert!(start.elapsed().as_secs() < 30, "画面への要求が終わらない");
            std::thread::yield_now();
        }
        request.join().unwrap().unwrap()
    };
    h.run();
    let clicked = ask(&mut h, "ui_click", json!({"target": "▶  実行"}));
    assert_eq!(clicked["ok"], true, "{clicked}");
    let start = std::time::Instant::now();
    while h.state().run_state(id).unwrap().tone == RunTone::Busy {
        h.input_mut().time = Some(clock.get());
        h.step();
        assert!(
            start.elapsed().as_secs() < 120,
            "固有振動の計算が終わらない"
        );
        std::thread::yield_now();
    }
    // 接続の処理と再生は有限のコマで確かめ、完了待ちの描き直しを待ち切ろうとしない。
    for _ in 0..2 {
        h.input_mut().time = Some(clock.get());
        h.step();
    }
    assert_eq!(h.state().run_state(id).unwrap().tone, RunTone::Done);
    let bar = h.state().frame_bar().expect("固有振動の帯");
    assert!(bar.playable);
    let before = h.state().overlay().shape.unwrap().mesh.positions.clone();
    let data = h.state().document().data().clone();
    assert!(
        h.query_by_label("再生").is_some(),
        "解けた後の実ボタンが無い: {:?}",
        h.state().frame_bar()
    );
    let clicked = ask(&mut h, "ui_click", json!({"target": "再生"}));
    assert_eq!(clicked["ok"], true, "{clicked}");
    clock.set(100.25);
    h.input_mut().time = Some(clock.get());
    h.step();
    assert!(h.state().cur.phase_playback.as_ref().unwrap().playing);
    assert_ne!(h.state().overlay().shape.unwrap().mesh.positions, before);
    let clicked = ask(&mut h, "ui_click", json!({"target": "一時停止"}));
    assert_eq!(clicked["ok"], true, "{clicked}");
    assert!(!h.state().cur.phase_playback.as_ref().unwrap().playing);
    let stopped = h.state().overlay().shape.unwrap().mesh.positions.clone();
    for time in [100.5, 100.75] {
        clock.set(time);
        h.input_mut().time = Some(time);
        h.step();
    }
    assert_eq!(h.state().overlay().shape.unwrap().mesh.positions, stopped);
    let clicked = ask(&mut h, "ui_click", json!({"target": "最初のコマ"}));
    assert_eq!(clicked["ok"], true, "{clicked}");
    assert_eq!(h.state().frame_bar().unwrap().at, 0);
    assert_eq!(h.state().document().data(), &data);
    h.state_mut().live_disconnect();
    std::fs::remove_dir_all(dir).unwrap();
}
