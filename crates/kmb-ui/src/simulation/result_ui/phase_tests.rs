use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_core::{Arg, Context, Extension, FieldUnit, Outcome};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Probe(Arc<Mutex<State>>);
struct State {
    phase: f64,
    frame: usize,
    revision: u64,
    enabled: bool,
    timed: bool,
}
impl Default for Probe {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(State {
            phase: 1.0,
            frame: 0,
            revision: 1,
            enabled: true,
            timed: false,
        })))
    }
}
impl Probe {
    fn prefix(s: &State) -> String {
        format!("probe.oscillation.{}.", s.revision)
    }
}
impl Extension for Probe {
    fn actions(&self, _: &Context<'_>) -> Vec<kmb_core::Action> {
        Vec::new()
    }
    fn unlisted(&self, id: &str) -> Option<Vec<Arg>> {
        let s = self.0.lock().unwrap();
        id.strip_prefix(&Self::prefix(&s))?.parse::<usize>().ok()?;
        Some(if s.timed {
            Vec::new()
        } else {
            vec![Arg::number("phase", "変位の係数", FieldUnit::Unitless).default(1.0)]
        })
    }
    fn frames(&self, _: &Context<'_>) -> Option<FrameBar> {
        let s = self.0.lock().unwrap();
        s.enabled.then(|| FrameBar {
            labels: vec!["甲".into(), "乙".into()],
            at: s.frame,
            total: "終".into(),
            playable: true,
            fps: if s.timed { 12.0 } else { 30.0 },
            seek: Self::prefix(&s),
        })
    }
    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        let mut s = self.0.lock().unwrap();
        let frame = id.strip_prefix(&Self::prefix(&s))?.parse::<usize>().ok()?;
        s.frame = frame;
        if !s.timed {
            s.phase = cx.arg("phase").and_then(|p| p.as_f64()).unwrap_or(1.0);
        }
        Some(Ok(Outcome::Message(String::new())))
    }
}

fn harness(step: f32) -> (Harness<'static, Workspace>, Probe) {
    let probe = Probe::default();
    let modules = kmb_bundle::modules().with_extension(probe.clone());
    let mut ws = Workspace::new(kmb_core::Document::new(), None, modules);
    ws.switch_space(crate::layout::Space::Simulation);
    let mut h = Harness::builder()
        .with_step_dt(step)
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    (h, probe)
}

#[test]
fn a_declared_phase_bar_plays_pauses_and_resets_without_changing_the_document() {
    for step in [0.25, 1.0 / 15.0, 1.0 / 60.0] {
        let (mut h, probe) = harness(step);
        let data = h.state().document().data().clone();
        let history = h.state().document().undo_labels().len();
        assert!(h.state().phase_bar(&h.state().frame_bar().unwrap()));
        h.get_by_label("再生").click();
        h.step();
        assert!(h.state().cur.phase_playback.as_ref().unwrap().playing);
        h.step();
        assert!(probe.0.lock().unwrap().phase < 1.0);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let out = requests.clone();
        h.ctx
            .set_request_repaint_callback(move |r| out.lock().unwrap().push(r.delay));
        h.step();
        assert!(
            requests
                .lock()
                .unwrap()
                .iter()
                .any(|d| !d.is_zero() && *d < std::time::Duration::from_millis(40))
        );
        h.get_by_label("一時停止").click();
        h.step();
        assert!(!h.state().cur.phase_playback.as_ref().unwrap().playing);
        let phase = probe.0.lock().unwrap().phase;
        h.run();
        h.step();
        assert_eq!(probe.0.lock().unwrap().phase, phase);
        h.get_by_label("再生").click();
        h.step();
        h.step();
        assert_ne!(probe.0.lock().unwrap().phase, phase);
        h.get_by_label("次のコマ").click();
        h.step();
        assert_eq!(probe.0.lock().unwrap().frame, 1);
        assert_eq!(probe.0.lock().unwrap().phase, 1.0);
        assert!(!h.state().cur.phase_playback.as_ref().unwrap().playing);
        h.run();
        h.get_by_label("再生").click();
        h.step();
        h.step();
        h.get_by_label("最初のコマ").click();
        h.step();
        assert_eq!(probe.0.lock().unwrap().frame, 0);
        assert_eq!(probe.0.lock().unwrap().phase, 1.0);
        // 既に最初のコマでも、最初のボタンで最大の形へ戻す。
        h.run();
        h.get_by_label("再生").click();
        h.step();
        h.step();
        h.get_by_label("最初のコマ").click();
        h.step();
        assert_eq!(probe.0.lock().unwrap().phase, 1.0);
        assert_eq!(h.state().document().data(), &data);
        assert_eq!(h.state().document().undo_labels().len(), history);
        assert!(!h.state().cur.modified);
    }
}

#[test]
fn a_changed_opaque_seek_or_missing_bar_stops_the_phase_clock() {
    let (mut h, probe) = harness(0.25);
    h.get_by_label("再生").click();
    h.step();
    h.step();
    probe.0.lock().unwrap().revision += 1;
    h.step();
    assert!(!h.state().cur.phase_playback.as_ref().unwrap().playing);
    assert_eq!(probe.0.lock().unwrap().phase, 1.0);
    h.get_by_label("再生").click();
    h.step();
    probe.0.lock().unwrap().enabled = false;
    h.step();
    assert!(h.state().cur.phase_playback.is_none());
    assert!(h.state().cur.frames_playing.is_none());
}

#[test]
fn an_ordinary_timed_bar_keeps_advancing_frames_and_stops_at_the_end() {
    let (mut h, probe) = harness(0.25);
    probe.0.lock().unwrap().timed = true;
    h.step();
    assert!(!h.state().phase_bar(&h.state().frame_bar().unwrap()));
    h.get_by_label("再生").click();
    h.step();
    h.step();
    assert_eq!(probe.0.lock().unwrap().frame, 1);
    assert!(h.state().cur.frames_playing.is_none());
    assert!(h.state().cur.phase_playback.is_none());
}

#[test]
fn phase_clocks_belong_to_their_document_tabs() {
    let (mut h, _) = harness(0.25);
    let first = h.state().tab_uid();
    h.get_by_label("再生").click();
    h.step();
    h.step();
    assert!(h.state().cur.phase_playback.as_ref().unwrap().playing);
    h.state_mut().new_tab();
    h.step();
    assert_ne!(h.state().tab_uid(), first);
    assert!(!h.state().cur.phase_playback.as_ref().unwrap().playing);
    assert_eq!(h.state().cur.phase_playback.as_ref().unwrap().angle, 0.0);
    h.state_mut().switch_tab(0);
    h.step();
    assert_eq!(h.state().tab_uid(), first);
    assert!(h.state().cur.phase_playback.as_ref().unwrap().playing);
    h.get_by_label("一時停止").click();
    h.step();
    h.run();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let out = requests.clone();
    h.ctx
        .set_request_repaint_callback(move |r| out.lock().unwrap().push(r.delay));
    h.step();
    assert!(
        !requests
            .lock()
            .unwrap()
            .iter()
            .any(|d| !d.is_zero() && *d < std::time::Duration::from_millis(40)),
        "停止中に揺れの予約を追加した"
    );
}
