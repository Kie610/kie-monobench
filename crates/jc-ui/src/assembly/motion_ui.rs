//! モーション スタディの窓。
//! ジョイントごとの行に、0〜100 のステップの帯とキーの点を出す。帯を押すと再生の頭がそのステップへ
//! 動き、値を入れて「キーを置く」でそのステップにキーを置く。再生は 1 回・繰り返し・往復。
//! 動かすのは 3D のビューの網だけで、形と値は変えない(jc_interchange::motion::body_poses)。キーと
//! スタディは文書に残り、取り消せる。スタディを変えるのは、ツールバーのボタンと同じモーション
//! スタディの操作([`jc_interchange::role::action::MOTION_STUDY`]。CLI・MCP と同じ道)。

use crate::Workspace;
use crate::layout::Space;
use crate::theme;
use jc_core::FeatureId;
use jc_interchange::animation::drive_of;
use jc_interchange::motion::{self, MotionStudy, STUDY_STEPS};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::Instant;

/// 再生のしかた。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PlayMode {
    /// 終わりで止まる
    #[default]
    Once,
    /// 終わったら始めから
    Loop,
    /// 終わったら逆向きに戻る
    PingPong,
}

impl PlayMode {
    const ALL: [PlayMode; 3] = [PlayMode::Once, PlayMode::Loop, PlayMode::PingPong];

    fn label(self) -> &'static str {
        match self {
            PlayMode::Once => "1 回",
            PlayMode::Loop => "繰り返し",
            PlayMode::PingPong => "往復",
        }
    }
}

/// 前に動かした時の(ステップ、スタディ、見えている形)。
type PoseKey = (u64, MotionStudy, Vec<(FeatureId, jc_core::ShapeHandle)>);

/// 窓の様子(文書ごと)。
#[derive(Debug, Default)]
pub(crate) struct MotionPage {
    pub open: bool,
    /// 見ているスタディの番号(並びの何番目)
    pub study: usize,
    /// 再生の頭(ステップ。途中の値もある)
    pub step: f64,
    /// 再生中なら前のコマの時刻
    pub playing: Option<Instant>,
    /// 往復の向き(1 か -1)
    pub dir: f64,
    pub mode: PlayMode,
    /// 打っている途中の値(ジョイントごと。見せる単位)。頭を動かすと消す
    pub typed: BTreeMap<FeatureId, f64>,
    /// 名前を変えている途中の名前
    pub renaming: Option<String>,
    /// 3D のビューを動かしているか
    pub posed: bool,
    /// 前に動かした時、動くボディがあったか
    posed_any: bool,
    /// 前に動かした時の(ステップ、スタディ、形の版)。同じなら動かし直さない
    pose_key: Option<PoseKey>,
}

/// 再生の頭を `dt` 秒進める。終わったら(1 回の時)偽を返す。
fn advance(step: &mut f64, dir: &mut f64, mode: PlayMode, dt: f64, seconds: f64) -> bool {
    let n = STUDY_STEPS as f64;
    let d = if *dir < 0.0 { -1.0 } else { 1.0 };
    let mut s = *step + d * dt * n / seconds.max(1e-3);
    let mut going = true;
    match mode {
        PlayMode::Once => {
            if s >= n {
                s = n;
                going = false;
            }
        }
        PlayMode::Loop => {
            if s > n {
                s = s.rem_euclid(n);
            }
        }
        PlayMode::PingPong => {
            if s > n {
                s = 2.0 * n - s;
                *dir = -1.0;
            } else if s < 0.0 {
                s = -s;
                *dir = 1.0;
            }
        }
    }
    *step = s.clamp(0.0, n);
    going
}

impl Workspace {
    /// 値で動かせるジョイント(回転・スライダ)と、その名前。タイムラインの順。
    fn drivable_joints(&self) -> Vec<(FeatureId, String)> {
        self.cur
            .work
            .doc()
            .data()
            .features
            .iter()
            .filter(|f| drive_of(self.cur.work.report(), f.id).is_some())
            .map(|f| (f.id, f.name.clone()))
            .collect()
    }

    /// 文書のモーション スタディ(区画 `asm.motion`)。
    pub(crate) fn motion_studies(&self) -> Vec<MotionStudy> {
        motion::studies(self.cur.work.doc().data())
    }

    /// 窓を開く(ツールバーのモーション スタディ)。スタディが無ければ操作で 1 つ作る。
    pub(crate) fn open_motion_study(&mut self) {
        if self.motion_studies().is_empty() && !self.motion_run(json!({})) {
            return;
        }
        self.cur.motion.open = true;
        self.cur.motion.study = self
            .cur
            .motion
            .study
            .min(self.motion_studies().len().saturating_sub(1));
        log::info!("モーション スタディの窓を開く");
    }

    /// モーション スタディの操作を引数 `args` で流す(見ているスタディの名前を足す)。文書が
    /// 変わったら真。
    pub(crate) fn motion_run(&mut self, mut args: Value) -> bool {
        let Some(id) = self.action_for_role(&jc_interchange::role::action::MOTION_STUDY) else {
            self.win.fail("モーション スタディの操作がありません");
            return false;
        };
        if let (Some(o), Some(s)) = (
            args.as_object_mut(),
            self.motion_studies().get(self.cur.motion.study),
        ) && !o.contains_key("study")
        {
            o.insert("study".into(), json!(s.name));
        }
        let before = self.motion_studies();
        self.run_action_args(&id, args);
        self.motion_studies() != before
    }

    /// 窓を開いている間に 3D のビューを動かしているか(アニメーションの片付けが消さないように)。
    pub(crate) fn motion_posing(&self) -> bool {
        self.cur.motion.posed
    }

    /// 窓を開いている間、コマごとに再生を進め、ボディを再生の頭の位置へ動かす。
    /// [`Workspace::sync_joint_preview`] の後に毎フレーム呼ぶ。
    pub(crate) fn sync_motion_study(&mut self, ctx: &egui::Context) {
        let study = (self.cur.motion.open && self.win.layout.space == Space::Design)
            .then(|| self.motion_studies().get(self.cur.motion.study).cloned())
            .flatten();
        let Some(study) = study else {
            self.cur.motion.playing = None;
            if std::mem::take(&mut self.cur.motion.posed) {
                self.cur.motion.pose_key = None;
                if self.win.layout.space != Space::Animation && !self.joint_previewing(ctx) {
                    self.cur.scene.set_poses(Default::default());
                }
            }
            return;
        };
        if let Some(prev) = self.cur.motion.playing {
            let now = Instant::now();
            let going = advance(
                &mut self.cur.motion.step,
                &mut self.cur.motion.dir,
                self.cur.motion.mode,
                (now - prev).as_secs_f64(),
                study.seconds,
            );
            self.cur.motion.playing = going.then_some(now);
            self.cur.motion.typed.clear();
            ctx.request_repaint();
        }
        let key = (
            self.cur.motion.step.to_bits(),
            study,
            self.cur.scene.key().to_vec(),
        );
        // 前と同じなら動かし直さない(ほかの所が動きを消していたら動かし直す)
        if self.cur.motion.pose_key.as_ref() == Some(&key)
            && (self.cur.scene.is_posed() || !self.cur.motion.posed_any)
        {
            return;
        }
        let poses = motion::body_poses(
            self.cur.work.doc().data(),
            &self.win.registry,
            self.cur.work.report(),
            &key.1,
            self.cur.motion.step,
        );
        self.cur.motion.posed_any = !poses.is_empty();
        self.cur.scene.set_poses(poses);
        self.cur.motion.posed = true;
        self.cur.motion.pose_key = Some(key);
    }

    /// ブラウザの「モーション スタディ」の中の行。押すとそのスタディの窓を開く。
    pub(crate) fn motion_study_rows(&mut self, ui: &mut egui::Ui) {
        let names: Vec<String> = self
            .motion_studies()
            .iter()
            .map(|s| s.name.clone())
            .collect();
        for (i, name) in names.iter().enumerate() {
            let r = crate::browser_ui::plain_row(ui, 2, "joint", name)
                .on_hover_text("押すとこのスタディの窓を開く");
            if r.clicked() {
                if self.cur.motion.study != i {
                    self.cur.motion.step = 0.0;
                    self.cur.motion.typed.clear();
                }
                self.cur.motion.study = i;
                if self.win.layout.space != Space::Design {
                    self.switch_space(Space::Design);
                }
                self.cur.motion.open = true;
                log::info!("ブラウザからモーション スタディ「{name}」を開く");
            }
        }
    }

    /// モーション スタディの窓。
    pub(crate) fn motion_study_window(&mut self, ctx: &egui::Context) {
        if !self.cur.motion.open {
            return;
        }
        if self.win.layout.space != Space::Design {
            self.cur.motion.open = false;
            return;
        }
        let list = self.motion_studies();
        if list.is_empty() {
            // 取り消しでスタディが無くなった
            self.cur.motion.open = false;
            return;
        }
        self.cur.motion.study = self.cur.motion.study.min(list.len() - 1);
        let study = list[self.cur.motion.study].clone();
        let joints = self.drivable_joints();
        let mut open = true;
        egui::Window::new("モーション スタディ")
            .id(egui::Id::new("motion_study_window"))
            .open(&mut open)
            .default_size([560.0, 300.0])
            .default_pos([300.0, 160.0])
            .show(ctx, |ui| {
                self.motion_header_ui(ui, &list);
                ui.separator();
                self.motion_play_ui(ui, &study);
                ui.separator();
                self.motion_rows_ui(ui, &study, &joints);
            });
        if !open {
            log::info!("モーション スタディの窓を閉じる");
            self.cur.motion.open = false;
            self.cur.motion.playing = None;
        }
    }

    /// スタディを選ぶ・作る・名前を変える・消す。
    fn motion_header_ui(&mut self, ui: &mut egui::Ui, list: &[MotionStudy]) {
        ui.horizontal(|ui| {
            let cur = &list[self.cur.motion.study].name;
            let mut pick = self.cur.motion.study;
            egui::ComboBox::from_id_salt("motion_study_pick")
                .selected_text(cur.as_str())
                .show_ui(ui, |ui| {
                    for (i, s) in list.iter().enumerate() {
                        ui.selectable_value(&mut pick, i, &s.name);
                    }
                });
            if pick != self.cur.motion.study {
                self.cur.motion.study = pick;
                self.cur.motion.step = 0.0;
                self.cur.motion.playing = None;
                self.cur.motion.typed.clear();
            }
            if ui
                .button("新しいスタディ")
                .on_hover_text("ジョイントの無いスタディを足す")
                .clicked()
                && self.motion_run(json!({"study": motion::free_name(list)}))
            {
                self.cur.motion.study = list.len();
                self.cur.motion.step = 0.0;
            }
            match self.cur.motion.renaming.clone() {
                Some(mut name) => {
                    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(140.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        || ui.button("OK").clicked()
                    {
                        self.cur.motion.renaming = None;
                        let name = name.trim().to_string();
                        if !name.is_empty() && &name != cur {
                            self.motion_run(json!({"rename": name}));
                        }
                    } else {
                        self.cur.motion.renaming = Some(name);
                    }
                }
                None => {
                    if ui.button("名前変更").clicked() {
                        self.cur.motion.renaming = Some(cur.clone());
                    }
                }
            }
            if ui
                .button("スタディを消す")
                .on_hover_text("見ているスタディを消す(元に戻すで戻せる)")
                .clicked()
                && self.motion_run(json!({"delete": true}))
            {
                self.cur.motion.study = self.cur.motion.study.saturating_sub(1);
            }
        });
    }

    /// 再生・再生のしかた・長さ・再生の頭。
    fn motion_play_ui(&mut self, ui: &mut egui::Ui, study: &MotionStudy) {
        ui.horizontal(|ui| {
            let playing = self.cur.motion.playing.is_some();
            if ui
                .button(if playing {
                    "■ 止める"
                } else {
                    "▶ 再生"
                })
                .clicked()
            {
                if playing {
                    self.cur.motion.playing = None;
                } else {
                    if self.cur.motion.mode == PlayMode::Once
                        && self.cur.motion.step >= STUDY_STEPS as f64
                    {
                        self.cur.motion.step = 0.0;
                    }
                    if self.cur.motion.dir == 0.0 {
                        self.cur.motion.dir = 1.0;
                    }
                    log::info!(
                        "モーション スタディ「{}」を再生({})",
                        study.name,
                        self.cur.motion.mode.label()
                    );
                    self.cur.motion.playing = Some(Instant::now());
                }
            }
            let mut mode = self.cur.motion.mode;
            egui::ComboBox::from_label("再生のしかた")
                .selected_text(mode.label())
                .show_ui(ui, |ui| {
                    for m in PlayMode::ALL {
                        ui.selectable_value(&mut mode, m, m.label());
                    }
                });
            self.cur.motion.mode = mode;
            let label = ui.label("長さ");
            let mut secs = study.seconds;
            let r = ui
                .add(
                    egui::DragValue::new(&mut secs)
                        .range(0.1..=600.0)
                        .speed(0.1)
                        .suffix(" 秒"),
                )
                .labelled_by(label.id);
            if (r.changed() && !r.dragged() || r.drag_stopped())
                && (secs - study.seconds).abs() > 1e-9
            {
                self.motion_run(json!({"seconds": secs}));
            }
        });
        ui.horizontal(|ui| {
            let label = ui.label("ステップ");
            // 再生中の頭は途中の値なので、整数に丸めて見せる(丸めただけでは止めない)
            let shown = self.cur.motion.step.round();
            let mut s = shown;
            ui.spacing_mut().slider_width = 240.0;
            let r = ui
                .add(egui::Slider::new(&mut s, 0.0..=STUDY_STEPS as f64).step_by(1.0))
                .labelled_by(label.id);
            if r.changed() && s != shown {
                self.cur.motion.step = s;
                self.cur.motion.playing = None;
                self.cur.motion.typed.clear();
            }
        });
    }

    /// ジョイントの行(帯とキー・今の値・キーを置く/消す)と、ジョイントを足す所。
    fn motion_rows_ui(
        &mut self,
        ui: &mut egui::Ui,
        study: &MotionStudy,
        joints: &[(FeatureId, String)],
    ) {
        let l = theme::look(ui.ctx());
        let step = self.cur.motion.step.round() as u32;
        let name_of = |id: FeatureId| {
            joints
                .iter()
                .find(|(j, _)| *j == id)
                .map_or_else(|| format!("機能 {}(動かせない)", id.0), |(_, n)| n.clone())
        };
        if study.tracks.is_empty() {
            ui.weak("下の「ジョイントを足す」で、動かすジョイントを選んでください。");
        }
        let mut action: Option<(&'static str, FeatureId, Option<f64>)> = None;
        egui::Grid::new("motion_rows")
            .num_columns(4)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                for t in &study.tracks {
                    let name = name_of(t.joint);
                    ui.label(&name);
                    // 帯: キーの点を結ぶ線と再生の頭。押すと頭がそのステップへ
                    let (rect, r) =
                        ui.allocate_exact_size(egui::vec2(240.0, 22.0), egui::Sense::click());
                    let r = r.on_hover_text("押すと再生の頭がそのステップへ動く");
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("{name} の帯"),
                        )
                    });
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, 3.0, l.pal.paper);
                    p.rect_stroke(
                        rect,
                        3.0,
                        egui::Stroke::new(1.0, l.pal.edge),
                        egui::StrokeKind::Inside,
                    );
                    let x_of = |s: f64| {
                        rect.left() + 6.0 + (rect.width() - 12.0) * (s / STUDY_STEPS as f64) as f32
                    };
                    let (lo, hi) = t.keys.iter().fold((f64::MAX, f64::MIN), |(a, b), k| {
                        (a.min(k.value), b.max(k.value))
                    });
                    let y_of = |v: f64| {
                        if hi - lo < 1e-9 {
                            rect.center().y
                        } else {
                            rect.bottom()
                                - 5.0
                                - (rect.height() - 10.0) * ((v - lo) / (hi - lo)) as f32
                        }
                    };
                    let pts: Vec<egui::Pos2> = t
                        .keys
                        .iter()
                        .map(|k| egui::pos2(x_of(k.step as f64), y_of(k.value)))
                        .collect();
                    if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
                        let mut line = vec![egui::pos2(x_of(0.0), a.y)];
                        line.extend(pts.iter().copied());
                        line.push(egui::pos2(x_of(STUDY_STEPS as f64), b.y));
                        p.add(egui::Shape::line(
                            line,
                            egui::Stroke::new(1.5, l.pal.design),
                        ));
                    }
                    for q in &pts {
                        p.circle_filled(*q, 3.5, l.pal.design);
                    }
                    let hx = x_of(self.cur.motion.step);
                    p.line_segment(
                        [egui::pos2(hx, rect.top()), egui::pos2(hx, rect.bottom())],
                        egui::Stroke::new(1.5, l.pal.sel),
                    );
                    if r.clicked()
                        && let Some(pos) = r.interact_pointer_pos()
                    {
                        let f =
                            ((pos.x - rect.left() - 6.0) / (rect.width() - 12.0)).clamp(0.0, 1.0);
                        self.cur.motion.step = (f as f64 * STUDY_STEPS as f64).round();
                        self.cur.motion.playing = None;
                        self.cur.motion.typed.clear();
                    }
                    // 値: 打っている途中の値か、頭の位置の値(キーが無ければジョイントの今の値)
                    let drive = drive_of(self.cur.work.report(), t.joint);
                    let unit = match &drive {
                        Some(d) if d.slide => " mm",
                        _ => "°",
                    };
                    let now = t
                        .value_at(self.cur.motion.step)
                        .or_else(|| drive.as_ref().map(|d| d.to_shown(d.value)))
                        .unwrap_or(0.0);
                    let mut v = self.cur.motion.typed.get(&t.joint).copied().unwrap_or(now);
                    let r = ui.add(egui::DragValue::new(&mut v).speed(1.0).suffix(unit));
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::DragValue,
                            true,
                            format!("{name} の値"),
                        )
                    });
                    if r.changed() {
                        self.cur.motion.typed.insert(t.joint, v);
                    }
                    ui.horizontal(|ui| {
                        let has_key = t.keys.iter().any(|k| k.step == step);
                        if ui
                            .button(format!("{name} のキーを置く"))
                            .on_hover_text(format!("ステップ {step} に、この値のキーを置く"))
                            .clicked()
                        {
                            action = Some(("put", t.joint, Some(v)));
                        }
                        if ui
                            .add_enabled(has_key, egui::Button::new(format!("{name} のキーを消す")))
                            .on_disabled_hover_text(format!("ステップ {step} にキーがありません"))
                            .clicked()
                        {
                            action = Some(("remove", t.joint, None));
                        }
                        if ui
                            .button(format!("{name} を外す"))
                            .on_hover_text("このジョイントをスタディから外す(キーも消える)")
                            .clicked()
                        {
                            action = Some(("drop", t.joint, None));
                        }
                    });
                    ui.end_row();
                }
            });
        let free: Vec<&(FeatureId, String)> = joints
            .iter()
            .filter(|(j, _)| study.tracks.iter().all(|t| t.joint != *j))
            .collect();
        ui.horizontal(|ui| {
            let mut add: Option<FeatureId> = None;
            ui.add_enabled_ui(!free.is_empty(), |ui| {
                egui::ComboBox::from_label("ジョイントを足す")
                    .selected_text("選ぶ")
                    .show_ui(ui, |ui| {
                        for (j, n) in &free {
                            if ui.selectable_label(false, n).clicked() {
                                add = Some(*j);
                            }
                        }
                    });
            });
            if let Some(j) = add {
                action = Some(("add", j, None));
            }
        });
        let Some((what, joint, value)) = action else {
            return;
        };
        let args = match what {
            "put" => json!({"joint": joint.0, "step": step, "value": value.unwrap_or(0.0)}),
            "remove" => json!({"joint": joint.0, "step": step, "remove": true}),
            "drop" => json!({"joint": joint.0, "drop": true}),
            // 足した時は、今の値のキーをステップ 0 に置く(すぐ動きを付けられるように)
            _ => json!({"joint": joint.0, "step": 0}),
        };
        if self.motion_run(args) {
            self.cur.motion.typed.remove(&joint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playing_stops_loops_or_turns_back_at_the_end() {
        // 4 秒で 100 ステップ: 1 秒で 25 進む
        let (mut s, mut d) = (90.0, 1.0);
        assert!(!advance(&mut s, &mut d, PlayMode::Once, 1.0, 4.0));
        assert_eq!(s, 100.0);
        let (mut s, mut d) = (90.0, 1.0);
        assert!(advance(&mut s, &mut d, PlayMode::Loop, 1.0, 4.0));
        assert!((s - 15.0).abs() < 1e-9);
        let (mut s, mut d) = (90.0, 1.0);
        assert!(advance(&mut s, &mut d, PlayMode::PingPong, 1.0, 4.0));
        assert!((s - 85.0).abs() < 1e-9);
        assert_eq!(d, -1.0);
        let mut s = 10.0;
        advance(&mut s, &mut d, PlayMode::PingPong, 1.0, 4.0);
        assert!((s - 15.0).abs() < 1e-9);
        assert_eq!(d, 1.0);
        assert_eq!(
            motion::free_name(&[MotionStudy::new("モーション スタディ1")]),
            "モーション スタディ2"
        );
    }
}
