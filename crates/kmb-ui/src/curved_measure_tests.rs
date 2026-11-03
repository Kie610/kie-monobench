//! 確かめること: 画面の台本から球面を測り、表示言語の半径と直径を読む。

use crate::*;
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn call(h: &mut Harness<'static, Workspace>, dir: &Path, tool: &str, args: Value) {
    let dir = dir.to_owned();
    let tool = tool.to_owned();
    let worker = std::thread::spawn(move || kmb_session::live::call(&dir, &tool, args));
    let began = Instant::now();
    while !worker.is_finished() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "画面の答えの時間切れ"
        );
    }
    let reply = worker.join().unwrap().unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    while h.state().is_busy() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "再計算の時間切れ"
        );
    }
    h.run_steps(8);
}

#[test]
fn curved_measure_live_spherical_face_has_radius_in_each_display_language() {
    for language in ["ja", "en"] {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = Scratch(
            std::env::temp_dir().join(format!("kmb-curved-measure-{}-{nonce}", std::process::id())),
        );
        std::fs::create_dir_all(&dir.0).unwrap();
        let mut ws = Workspace::new(
            kmb_samples::corner_document("solid.fillet@1", "ball"),
            None,
            crate::tests::reg(),
        )
        .with_data_dir(dir.0.clone());
        ws.layout_mut().ai_live = true;
        ws.layout_mut().appearance.language = language.into();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1400.0, 800.0))
            .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
        h.run();
        call(&mut h, &dir.0, "ui_pick", json!({"face":"f3:side:ball"}));
        assert_eq!(h.state().picked_faces()[0].face, "f3:side:ball");
        call(&mut h, &dir.0, "ui_key", json!({"keys":"I"}));
        assert!(h.state().is_measuring());
        let label = if language == "ja" { "半径" } else { "Radius" };
        let diameter = if language == "ja" {
            "直径"
        } else {
            "Diameter"
        };
        assert!(h.query_by_label(label).is_some(), "{language}: {label}");
        assert!(
            h.query_by_label(diameter).is_some(),
            "{language}: {diameter}"
        );
        let sphere = h.state().cur.measure.as_ref().unwrap();
        assert!(sphere.report.rows.iter().any(|(k, _)| k == "半径"));
    }
}
