//! 重い見本で、穴を続けて置く実際の UI フレームの停止時間を測る。通常の検査では実行しない。

use egui_kittest::Harness;
use kmb_ui::Workspace;
use std::time::{Duration, Instant};

struct Frames {
    name: &'static str,
    times: Vec<Duration>,
}
impl Frames {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            times: Vec::new(),
        }
    }
    fn step(&mut self, h: &mut Harness<'_, Workspace>) {
        let started = Instant::now();
        h.step();
        self.times.push(started.elapsed());
    }
    fn report(&mut self) {
        self.times.sort_unstable();
        println!(
            "{}: {} frames / median {:.3} ms / max {:.3} ms / total {:.3} ms",
            self.name,
            self.times.len(),
            self.times[self.times.len() / 2].as_secs_f64() * 1000.0,
            self.times.last().unwrap().as_secs_f64() * 1000.0,
            self.times.iter().sum::<Duration>().as_secs_f64() * 1000.0
        );
    }
}
fn script(h: &mut Harness<'_, Workspace>, text: &str, frames: &mut Frames) {
    let batch = h.state_mut().run_script(text).expect("台本を読める");
    let started = Instant::now();
    loop {
        frames.step(h);
        if let Some(result) = h.state_mut().script_result(batch) {
            assert!(result.ok, "{}", result.report());
            return;
        }
        assert!(
            started.elapsed().as_secs() < 60,
            "台本の時間切れ: {:?}",
            h.state().command_log()
        );
    }
}
fn main() {
    let count: usize = std::env::args()
        .nth(1)
        .map(|s| s.parse().expect("穴の数は整数"))
        .unwrap_or(8);
    assert!((2..=32).contains(&count));
    let part = kmb_samples::reference_part();
    let face = format!("f{}:end", part.plate.0);
    let mut doc = part.doc;
    let mut data = doc.data().feature(part.hole_pattern).unwrap().data.clone();
    data["count"] = serde_json::json!(count.to_string());
    data["spacing"] = serde_json::json!("6 mm");
    let tx = kmb_session::edit::set_feature_data(doc.data(), part.hole_pattern, data);
    doc.apply(tx).unwrap();
    let mut ws = Workspace::open(doc, None, true, kmb_bundle::modules());
    ws.wait();
    assert_eq!(ws.report().error_count(), 0);
    println!(
        "見本: 基準部品 / 穴パターン2列×{count} / {} features / {} visible bodies / background=true",
        ws.document().data().features.len(),
        ws.visible_bodies().len()
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(1920.0, 1080.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    kmb_ui::install_fonts(&h.ctx, None);
    for _ in 0..4 {
        h.run();
    }
    let mut idle = Frames::new("操作前のフレーム");
    for _ in 0..8 {
        idle.step(&mut h);
    }
    idle.report();
    let mut setup = Frames::new("面を選び穴を始める");
    script(
        &mut h,
        &format!("pick {face}\nclick \"穴\"\npick {face}"),
        &mut setup,
    );
    let before = h.state().document().data().features.len();
    h.event(egui::Event::Text("2".into()));
    h.step();
    h.key_press(egui::Key::Enter);
    let mut then = Frames::new("径を決めて次の穴へ進む");
    then.step(&mut h);
    assert_eq!(
        h.state().document().data().features.len(),
        before + 2,
        "穴と位置のスケッチを作る"
    );
    assert_eq!(h.state().report().error_count(), 0);
    then.report();
    for i in 0..3 {
        let previous = h.state().document().to_json();
        let mut repeat = Frames::new("続けて同じ穴を足す");
        script(&mut h, &format!("pick {face}"), &mut repeat);
        h.state_mut().wait();
        assert_ne!(h.state().document().to_json(), previous, "{i}: 点が増える");
        assert_eq!(h.state().document().data().features.len(), before + 2);
        assert_eq!(h.state().report().error_count(), 0);
        repeat.report();
    }
    let mut finish = Frames::new("Enterで終える");
    script(&mut h, "key Enter", &mut finish);
    assert!(!h.state().script_running());
    println!(
        "結果: {} features / errors {}",
        h.state().document().data().features.len(),
        h.state().report().error_count()
    );
}
