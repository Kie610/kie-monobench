//! 作業空間「アニメーション」の画面: 下のストーリーボード(段の帯)と、3D のビューで段の動きを見せる・再生する。
//!
//! 段の中身と動かし方は [`kmb_interchange::animation`]。段を足すのはリボンの「分解の移動」「自動分解」
//! 「ジョイントの動き」(kmb-asm)で、ここでは時刻を動かし、段の値と長さを直し、並べ替え・消す。3D の
//! ビューは今の時刻の位置にボディを動かして描く(形は変えない。ほかの作業空間では動かさない)。
//! 跡の線(トレイルライン)の印を持つ分解の段は、元の位置から今の位置への線を重ねる。
//!
//! ビューの段があれば、時刻を動かすたびに 3D のビューの視点をその時刻の視点にする(自由に回した後も、
//! 時刻を動かせば戻る)。段の帯の「記録」を入れている間は、視点を動かして
//! 止めるたびにビューの段を足す。表示/非表示・外観の段は、その時刻の隠れ方と色で描く(文書の
//! 表示と外観は変えない)。
//!
//! 自動分解はすぐ実行せず、ビューの上に小さな道具(ワンステップ/
//! 順次、トレイルライン、分解の目盛、OK・やめる)を出し、変えるたびに操作を実行し直して先に見せる。

use crate::Workspace;
use crate::layout::Space;
use crate::theme;
use egui::{Sense, Stroke, Vec2};
use kmb_core::SectionType;
use kmb_core::{Edit, FeatureId, LineKind, Outcome};
use kmb_interchange::animation::{self as anim, Step, StepKind, Storyboard};
use std::time::Instant;

/// 動きを計算した時の鍵(時刻・段・ボディの形)。
type PoseKey = (u64, Vec<Step>, Vec<(FeatureId, kmb_core::ShapeHandle)>);

/// ストーリーボードの状態(画面の状態。文書には入れない)。
#[derive(Default)]
pub(crate) struct AnimState {
    /// 今の時刻(秒)
    pub(crate) time: f64,
    /// 再生している時の、前のフレームの時刻
    playing: Option<Instant>,
    /// 前に見た段の数(段を足したら、その段の終わりへ時刻を進める)。初めは `None`
    seen: Option<usize>,
    /// 値を引いている途中の段(離すと文書に入れる)
    draft: Option<Vec<Step>>,
    /// 前に動きを計算した時の鍵(時刻・段・ボディの形)
    key: Option<PoseKey>,
    /// 自動分解の小さな道具(出している間だけ)
    pub(crate) auto: Option<AutoTool>,
    /// 前に計算した跡の線と、その鍵
    trails: Option<(PoseKey, Vec<Vec<[f64; 3]>>)>,
    /// ビューの記録(入れている間は、視点を動かして止めるたびにビューの段を足す)
    pub(crate) recording: bool,
    /// 記録の基準の視点(段の視点にした時か、前に記録した時の視点)
    base_view: Option<anim::ViewPose>,
    /// 基準から動いた視点と、そうなった時刻(しばらく止まったら記録する)
    moved: Option<(Instant, anim::ViewPose)>,
    /// 表示/非表示・外観の段で、3D のビューの隠れ方か色を替えているか(作業空間を出たら戻す)
    overrode: bool,
}

/// 視点を動かして止めてから、ビューの段を記録するまでの間(秒。ホイールのひと続きを 1 つにまとめる)。
pub(crate) const RECORD_SETTLE: f64 = 0.4;

/// 自動分解の小さな道具の状態。
pub(crate) struct AutoTool {
    /// 操作(1 レベルか、すべてのレベル)
    action: String,
    /// 道具を出した時に選んでいた部品。先に見せる間に選びが変わっても対象は保つ。
    component: Option<u64>,
    /// 順次(部品ごとに 1 秒の段)か、ワンステップ(全部同時の段 1 つ)か
    pub(crate) sequential: bool,
    /// 跡の線を出すか
    pub(crate) trail: bool,
    /// 分解の目盛(距離の倍率)
    pub(crate) scale: f64,
    /// 先に見せている段(前からの段と足した段の全部。OK で文書に入れる)
    preview: Vec<Step>,
    /// 取り消しの名前
    label: String,
    /// 道具を出す前の時刻(やめた時に戻す)
    before: f64,
}

/// 分解の目盛の範囲(距離の倍率)。
const SCALE_RANGE: std::ops::RangeInclusive<f64> = 0.1..=3.0;

impl Workspace {
    /// ストーリーボードの今の時刻(秒)。
    pub fn animation_time(&self) -> f64 {
        self.cur.anim.time
    }

    /// 今見せている段(自動分解の道具を出していればその先の見せ方、値を引いている途中ならその段、
    /// ほかは文書の段)。
    pub fn animation_steps_shown(&self) -> Vec<Step> {
        if let Some(a) = &self.cur.anim.auto {
            return a.preview.clone();
        }
        self.cur
            .anim
            .draft
            .clone()
            .unwrap_or_else(|| kmb_interchange::animation::steps(self.cur.work.doc().data()))
    }

    /// 自動分解の小さな道具を出しているか。
    pub fn is_auto_exploding(&self) -> bool {
        self.cur.anim.auto.is_some()
    }

    /// 自動分解の操作 `id` の小さな道具を出し、既定(ワンステップ・跡の線あり・目盛 1)で先に見せる。
    pub(crate) fn open_auto_explode(&mut self, id: &str) {
        self.cur.anim.playing = None;
        self.cur.anim.draft = None;
        self.cur.anim.auto = Some(AutoTool {
            action: id.into(),
            component: self.cur.selected_component,
            sequential: false,
            trail: true,
            scale: 1.0,
            preview: Vec::new(),
            label: String::new(),
            before: self.cur.anim.time,
        });
        log::info!("自動分解の道具を出す: {id}");
        if !self.refresh_auto_explode() {
            self.cur.anim.auto = None;
        }
    }

    /// 自動分解の道具の欄(順次・跡の線・目盛)を変える(画面の道具と試験が使う)。
    pub fn set_auto_explode(&mut self, sequential: bool, trail: bool, scale: f64) {
        let Some(a) = self.cur.anim.auto.as_mut() else {
            return;
        };
        a.sequential = sequential;
        a.trail = trail;
        a.scale = scale.clamp(*SCALE_RANGE.start(), *SCALE_RANGE.end());
        self.refresh_auto_explode();
    }

    /// 自動分解の操作を今の欄で実行し直し、足す段を先に見せる(時刻は足した段の終わり)。
    /// 実行できなければ知らせて `false`。
    fn refresh_auto_explode(&mut self) -> bool {
        let Some(a) = self.cur.anim.auto.as_ref() else {
            return false;
        };
        let args = serde_json::json!({
            "component": a.component,
            "sequential": a.sequential,
            "trail": a.trail,
            "scale": a.scale,
        });
        let id = a.action.clone();
        let r = {
            let cx = kmb_core::Context {
                args: &args,
                ..self.context()
            };
            self.win.extensions.run(&id, &cx)
        };
        let found = match r {
            Ok(Outcome::Change { label, edits }) => edits.into_iter().find_map(|e| match e {
                // 段の区画の変更(先に見せるだけで、文書へはまだ入れない)
                Edit::SetSection { key, section } if key == Storyboard::KEY => {
                    let steps = match section {
                        Some(s) => serde_json::from_value::<Storyboard>(s.data).ok()?.steps,
                        None => Vec::new(),
                    };
                    Some((label.clone(), steps))
                }
                _ => None,
            }),
            Ok(_) => None,
            Err(e) => {
                self.win.fail(e);
                return false;
            }
        };
        let Some((label, steps)) = found else {
            return false;
        };
        let total = anim::duration(&steps);
        if let Some(a) = self.cur.anim.auto.as_mut() {
            a.preview = steps;
            a.label = label;
        }
        self.cur.anim.time = total;
        true
    }

    /// 自動分解の道具の OK: 先に見せた段を文書に入れる(1 回の取り消し単位)。
    pub fn finish_auto_explode(&mut self) {
        if let Some(a) = self.cur.anim.auto.take() {
            let total = anim::duration(&a.preview);
            self.set_animation(&a.label, a.preview);
            self.cur.anim.time = total;
        }
    }

    /// 自動分解の道具をやめる(段は足さず、時刻を元に戻す)。
    pub fn cancel_auto_explode(&mut self) {
        if let Some(a) = self.cur.anim.auto.take() {
            log::info!("自動分解の道具をやめる");
            self.cur.anim.time = a.before;
        }
    }

    /// ストーリーボードの時刻を `t` 秒にする(初めから終わりの間に収める)。
    pub fn set_animation_time(&mut self, t: f64) {
        let total = anim::duration(&self.animation_steps_shown());
        self.cur.anim.time = t.clamp(0.0, total);
    }

    /// 再生しているか。
    pub fn is_animation_playing(&self) -> bool {
        self.cur.anim.playing.is_some()
    }

    /// 再生を始める(終わりにいれば初めから)・止める。
    pub fn toggle_animation_play(&mut self) {
        if self.cur.anim.playing.take().is_some() {
            log::info!("アニメーションの再生を止める({:.2} 秒)", self.cur.anim.time);
            return;
        }
        let total = anim::duration(&kmb_interchange::animation::steps(
            self.cur.work.doc().data(),
        ));
        if total <= 0.0 {
            self.win.message = Some(
                "段がありません。「分解の移動」「自動分解」か「ジョイントの動き」で足してください"
                    .into(),
            );
            return;
        }
        if self.cur.anim.time >= total - 1e-9 {
            self.cur.anim.time = 0.0;
        }
        log::info!("アニメーションを再生する({:.2} 秒から)", self.cur.anim.time);
        self.cur.anim.playing = Some(Instant::now());
    }

    /// 再生を `dt` 秒進める(試験と、画面のフレームごと)。終わりで止まる。
    pub fn advance_animation(&mut self, dt: f64) {
        let total = anim::duration(&kmb_interchange::animation::steps(
            self.cur.work.doc().data(),
        ));
        self.cur.anim.time = (self.cur.anim.time + dt).min(total);
        if self.cur.anim.time >= total {
            self.cur.anim.playing = None;
        }
    }

    /// 段を `steps` に置き換える(取り消せる)。
    pub fn set_animation(&mut self, label: &str, steps: Vec<Step>) -> bool {
        self.cur.anim.draft = None;
        if steps == kmb_interchange::animation::steps(self.cur.work.doc().data()) {
            return false;
        }
        log::info!("アニメーションの段を変える: {label}(段 {} 個)", steps.len());
        // 足した時の時刻の送りは段を足す操作の時だけ
        self.cur.anim.seen = Some(steps.len());
        let data = self.cur.work.doc().data();
        match kmb_session::edit::set_animation(data, label, steps) {
            Ok(tx) => self.apply(tx),
            Err(e) => {
                self.win.fail(e);
                false
            }
        }
    }

    /// 3D のビューの手順(分解の矢印など)に渡す今の時刻。
    pub(crate) fn animation_args(&self, args: &mut serde_json::Map<String, serde_json::Value>) {
        if self.win.layout.space == Space::Animation && !args.contains_key("time") {
            args.insert("time".into(), serde_json::json!(self.cur.anim.time));
        }
    }

    /// 3D のビューのボディを今の時刻の位置に動かす(アニメーションの作業空間だけ)。毎フレーム呼ぶ。
    pub(crate) fn sync_animation(&mut self, ctx: &egui::Context) {
        if self.win.layout.space != Space::Animation {
            self.cur.anim.playing = None;
            self.cur.anim.key = None;
            self.cur.anim.auto = None;
            self.cur.anim.recording = false;
            if std::mem::take(&mut self.cur.anim.overrode) {
                self.refresh_body_colors();
                self.cur.scene.set_hidden(&self.cur.hidden_now);
            }
            // ジョイントの動きを見ている間とモーション スタディの間と部品を引いている間は、そちらが
            // 動かす(joint_preview_ui・motion_ui・joint_drag_ui)
            if self.cur.scene.is_posed()
                && !self.joint_previewing(ctx)
                && !self.motion_posing()
                && self.cur.drag_pose.is_none()
            {
                self.cur.scene.set_poses(Default::default());
            }
            return;
        }
        let n = kmb_interchange::animation::steps(self.cur.work.doc().data()).len();
        let total = anim::duration(&self.animation_steps_shown());
        match self.cur.anim.seen {
            // 段が増えた(リボンの操作で足した): 足した段の終わり(今の最後)を見せる
            Some(s) if n > s => self.cur.anim.time = total,
            _ => {}
        }
        self.cur.anim.seen = Some(n);
        if let Some(prev) = self.cur.anim.playing {
            let now = Instant::now();
            self.cur.anim.playing = Some(now);
            self.advance_animation((now - prev).as_secs_f64());
            ctx.request_repaint();
        }
        self.cur.anim.time = self.cur.anim.time.clamp(0.0, total);
        let steps = self.animation_steps_shown();
        self.sync_shown_and_looks(&steps);
        let key = (
            self.cur.anim.time.to_bits(),
            steps,
            self.cur.scene.key().to_vec(),
        );
        if self.cur.anim.key.as_ref() == Some(&key) {
            self.record_view(ctx);
            return;
        }
        let poses = anim::body_poses(
            self.cur.work.doc().data(),
            &self.win.registry,
            self.cur.work.report(),
            &key.1,
            self.cur.anim.time,
        );
        self.cur.scene.set_poses(poses);
        if let Some(v) = anim::view_at(&key.1, self.cur.anim.time) {
            self.apply_step_view(&v);
        }
        self.cur.anim.key = Some(key);
    }

    /// 3D のビューの視点を、ビューの段の視点 `v` にする(すぐ。滑らかに替えない)。
    fn apply_step_view(&mut self, v: &anim::ViewPose) {
        self.cur.camera.place(
            v.target.map(|x| x as f32),
            v.yaw as f32,
            v.pitch as f32,
            v.height as f32,
            self.win.view_rect,
        );
        self.cur.camera.perspective = self.win.layout.projection.perspective_for(&self.cur.camera);
        self.cur.anim.base_view = Some(*v);
        self.cur.anim.moved = None;
    }

    /// 3D のビューの今の視点(ビューの段にする量)。
    pub(crate) fn view_pose(&self) -> anim::ViewPose {
        let (c, yaw, pitch, height) = self.cur.camera.snapshot(self.win.view_rect);
        anim::ViewPose {
            target: c.map(f64::from),
            yaw: f64::from(yaw),
            pitch: f64::from(pitch),
            height: f64::from(height),
            perspective: self.cur.camera.perspective,
        }
    }

    /// ビューの記録を入れる・切る。入れた時にビューの段が無ければ、今の視点を始めの段にする。
    pub fn set_view_recording(&mut self, on: bool) {
        self.cur.anim.recording = on;
        self.cur.anim.moved = None;
        if !on {
            log::info!("ビューの記録を切る");
            return;
        }
        let now = self.view_pose();
        self.cur.anim.base_view = Some(now);
        let any = kmb_interchange::animation::steps(self.cur.work.doc().data())
            .iter()
            .any(|s| matches!(s.kind, StepKind::View { .. }));
        log::info!(
            "ビューの記録を入れる(ビューの段 {})",
            if any { "あり" } else { "なし" }
        );
        if !any {
            self.add_view_step(now);
        }
    }

    /// 視点 `v` へ動かすビューの段を足す。
    fn add_view_step(&mut self, v: anim::ViewPose) {
        let Some(id) = self.action_for_role(&kmb_interchange::role::action::ANIMATION_VIEW) else {
            return;
        };
        let args = serde_json::json!({"view": v, "time": self.cur.anim.time});
        self.run_action_args(&id, args);
        self.cur.anim.base_view = Some(v);
    }

    /// 記録を入れている間、視点を動かして [`RECORD_SETTLE`] 秒止まったらビューの段を足す。
    fn record_view(&mut self, ctx: &egui::Context) {
        if !self.cur.anim.recording
            || self.cur.anim.playing.is_some()
            || self.cur.anim.auto.is_some()
        {
            return;
        }
        if ctx.input(|i| i.pointer.any_down()) || self.cur.camera.turning() {
            return;
        }
        let now = self.view_pose();
        if self.cur.anim.base_view.is_some_and(|b| b.near(&now)) {
            self.cur.anim.moved = None;
            return;
        }
        match self.cur.anim.moved {
            Some((at, v)) if v.near(&now) => {
                if at.elapsed().as_secs_f64() >= RECORD_SETTLE {
                    log::info!("ビューを記録する: {now:?}");
                    self.cur.anim.moved = None;
                    self.add_view_step(now);
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(RECORD_SETTLE));
                }
            }
            _ => {
                self.cur.anim.moved = Some((Instant::now(), now));
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(RECORD_SETTLE));
            }
        }
    }

    /// 表示/非表示・外観の段の、今の時刻の隠れ方と色を 3D のビューに出す(段が無ければ文書のまま)。
    fn sync_shown_and_looks(&mut self, steps: &[Step]) {
        let any = steps.iter().any(|s| {
            matches!(
                s.kind,
                StepKind::Visibility { .. } | StepKind::Appearance { .. }
            )
        });
        if !any {
            if std::mem::take(&mut self.cur.anim.overrode) {
                self.refresh_body_colors();
                self.cur.scene.set_hidden(&self.cur.hidden_now);
            }
            return;
        }
        self.cur.anim.overrode = true;
        let t = self.cur.anim.time;
        let data = self.cur.work.doc().data();
        let shown = anim::shown_at(data, steps, t);
        let blends = anim::appearances_at(data, steps, t);
        let roots = self.body_roots();
        let mut hidden = self.cur.hidden_now.clone();
        for (last, root) in &roots {
            match shown.get(root) {
                Some(false) => {
                    hidden.insert(*last);
                }
                Some(true) => {
                    hidden.remove(last);
                }
                None => {}
            }
        }
        self.cur.scene.set_hidden(&hidden);
        if blends.is_empty() {
            return;
        }
        let rgb = |c: egui::Color32| [c.r(), c.g(), c.b()];
        let steel = kmb_look::material::phys_material(kmb_look::material::DEFAULT_MATERIAL)
            .map_or([150, 150, 150], |m| m.color);
        // このデザイン内で作った外観も、ライブラリの外観と同じに色を引く
        let own = kmb_look::appearance::of(data);
        let color_of = |name: &str| own.resolve(name).map(|a| a.color);
        let body = crate::menus_ui::BodyLooks::read(data);
        let mut colors = std::collections::BTreeMap::new();
        let mut looks = std::collections::BTreeMap::new();
        for (last, root) in &roots {
            let own = body.color(*root);
            let Some(b) = blends.get(root) else {
                if let Some(c) = own {
                    colors.insert(*last, c);
                }
                continue;
            };
            let from = b
                .from
                .as_deref()
                .and_then(color_of)
                .or(own.map(rgb))
                .unwrap_or(steel);
            let to = color_of(&b.to).unwrap_or(from);
            let [r, g, bl] = b.mix(from, to);
            colors.insert(*last, egui::Color32::from_rgb(r, g, bl));
            if let Some(name) = b.shown().filter(|_| b.f <= 0.0 || b.f >= 1.0) {
                let m = body.materials.get(*root);
                looks.insert(*last, kmb_render::Look::named(Some(name), m));
            }
        }
        self.cur.scene.set_body_colors(colors);
        self.cur.scene.set_body_looks(looks);
    }

    /// 跡の線(トレイルライン)。アニメーションの作業空間で、跡の線の印を持つ分解の段で動いた
    /// ボディの、元の位置の中心から今の位置までの線。
    pub(crate) fn trail_lines(&mut self) -> Vec<(Vec<[f64; 3]>, LineKind)> {
        if self.win.layout.space != Space::Animation {
            return Vec::new();
        }
        let steps = self.animation_steps_shown();
        let key = (
            self.cur.anim.time.to_bits(),
            steps,
            self.cur.scene.key().to_vec(),
        );
        let lines = match &self.cur.anim.trails {
            Some((k, lines)) if *k == key => lines.clone(),
            _ => {
                // ボディ(最初に作った機能)ごとの、元の位置の外形の箱の中心
                let data = self.cur.work.doc().data();
                let mut boxes: std::collections::BTreeMap<FeatureId, ([f64; 3], [f64; 3])> =
                    Default::default();
                for (id, h) in
                    kmb_core::visible_bodies(data, &self.win.registry, self.cur.work.report())
                {
                    let Ok((a, b)) = self.cur.work.kernel().bounds(h) else {
                        continue;
                    };
                    let r = kmb_core::body_root(data, &self.win.registry, id);
                    let e = boxes.entry(r).or_insert((a, b));
                    for i in 0..3 {
                        e.0[i] = e.0[i].min(a[i]);
                        e.1[i] = e.1[i].max(b[i]);
                    }
                }
                let lines = anim::trails(
                    data,
                    &self.win.registry,
                    self.cur.work.report(),
                    &key.1,
                    self.cur.anim.time,
                    |r| {
                        boxes
                            .get(&r)
                            .map(|(a, b)| std::array::from_fn(|i| (a[i] + b[i]) / 2.0))
                    },
                );
                self.cur.anim.trails = Some((key, lines.clone()));
                lines
            }
        };
        lines
            .into_iter()
            .map(|l| {
                (
                    l,
                    LineKind::new(kmb_interchange::ink::line::FAINT, "トレイルライン"),
                )
            })
            .collect()
    }

    /// 自動分解の小さな道具(ビューの上の真ん中): 分解の仕方(ワンステップ/順次)の ▾、
    /// トレイルラインの入切、分解の目盛、OK、やめる。
    pub(crate) fn auto_explode_ui(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let Some(a) = self.cur.anim.auto.as_ref() else {
            return;
        };
        let (mut sequential, mut trail, mut scale) = (a.sequential, a.trail, a.scale);
        let l = theme::look(ui.ctx());
        let width = 360.0 * theme::band();
        let pos = egui::pos2(rect.center().x - width / 2.0, rect.top() + 46.0);
        let (mut ok, mut cancel) = (false, false);
        egui::Area::new(egui::Id::new("auto_explode_tool"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(l.pal.panel)
                    .stroke(Stroke::new(1.0, l.pal.ws(Space::Animation)))
                    .corner_radius(theme::corner(6.0))
                    .inner_margin(egui::Margin::symmetric(6, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            // 分解の仕方: 今の物のアイコンと ▾。一覧で選ぶ
                            let (icon, name) = mode_icon(sequential);
                            let r = theme::icon_button(ui, icon, name, true, false)
                                .on_hover_text(format!(
                                    "分解の仕方: {name}(クリックで選ぶ)"
                                ));
                            egui::Popup::menu(&r).show(|ui| {
                                for seq in [false, true] {
                                    let (icon, name) = mode_icon(seq);
                                    let hint = if seq {
                                        "部品を 1 つずつ順に動かす(部品ごとに 1 秒の段)"
                                    } else {
                                        "全部の部品を同時に動かす(段 1 つ)"
                                    };
                                    if theme::menu_item(ui, icon, name, true, seq == sequential)
                                        .on_hover_text(hint)
                                        .clicked()
                                    {
                                        sequential = seq;
                                        ui.close();
                                    }
                                }
                            });
                            if theme::icon_button(ui, "trail", "トレイルライン", true, trail)
                                .on_hover_text(if trail {
                                    "トレイルライン: 入(元の位置から今の位置への線を出す)。クリックで切る"
                                } else {
                                    "トレイルライン: 切。クリックで入れる(元の位置から今の位置への線)"
                                })
                                .clicked()
                            {
                                trail = !trail;
                            }
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new("分解の目盛").color(l.pal.ink2));
                            ui.spacing_mut().slider_width = 110.0 * theme::band();
                            ui.add(
                                egui::Slider::new(&mut scale, SCALE_RANGE)
                                    .show_value(false)
                                    .step_by(0.05),
                            )
                            .on_hover_text("部品を離す距離の倍率(1 で、同じ向きへ離す部品の大きさを積んだ距離)");
                            ui.label(
                                egui::RichText::new(format!("×{scale:.2}")).color(l.pal.ink2),
                            );
                            ui.add_space(4.0);
                            ok = theme::icon_button(ui, "check", "OK", true, false)
                                .on_hover_text("この分解を段の帯に足す(Enter)")
                                .clicked();
                            cancel = theme::icon_button(ui, "x", "キャンセル", true, false)
                                .on_hover_text("段を足さずにやめる(Esc)")
                                .clicked();
                        });
                    });
            });
        if ok {
            self.finish_auto_explode();
        } else if cancel {
            self.cancel_auto_explode();
        } else if let Some(a) = self.cur.anim.auto.as_ref()
            && (a.sequential != sequential || a.trail != trail || a.scale != scale)
        {
            self.set_auto_explode(sequential, trail, scale);
        }
    }

    /// 動かす物の名前(部品の名前か、ボディの名前)。
    fn target_name(&self, t: &anim::Target) -> String {
        match t {
            anim::Target::Component { id } => self
                .cur
                .work
                .doc()
                .data()
                .component(*id)
                .map_or_else(|| format!("部品 {id}"), |c| c.name.clone()),
            anim::Target::Body { root } => self.body_name_of(*root),
        }
    }

    /// 段の名前(「分解: 部品1 を Z へ 40 mm」など)。
    pub fn animation_step_label(&self, s: &Step) -> String {
        let data = self.cur.work.doc().data();
        match &s.kind {
            StepKind::ExplodeMany { moves } => {
                let names: Vec<String> = moves
                    .iter()
                    .filter_map(|m| m.targets.first().map(|t| self.target_name(t)))
                    .collect();
                let shown = if names.len() > 3 {
                    format!("{} ほか", names[..3].join("・"))
                } else {
                    names.join("・")
                };
                format!("分解(同時に {} 個): {shown}", moves.len())
            }
            StepKind::Explode { targets, offset } => {
                let names: Vec<String> = targets.iter().map(|t| self.target_name(t)).collect();
                let len = kmb_core::kernel::dot(*offset, *offset).sqrt();
                format!(
                    "分解: {} を {} へ {} mm",
                    names.join("・"),
                    direction_name(*offset),
                    num(len)
                )
            }
            StepKind::Drive { joint, from, to } => {
                let name = data
                    .feature(*joint)
                    .map_or_else(|| format!("機能 {}", joint.0), |f| f.name.clone());
                let unit = self.drive_unit(*joint);
                format!("動き: {name} {}{unit} → {}{unit}", num(*from), num(*to))
            }
            StepKind::View { view } => format!(
                "ビュー: 向き {}° / {}°・高さ {} mm",
                num(view.yaw.rem_euclid(360.0)),
                num(view.pitch),
                num(view.height)
            ),
            StepKind::Visibility { targets, visible } => format!(
                "{}: {}",
                if *visible { "表示" } else { "非表示" },
                self.target_names(targets)
            ),
            StepKind::Appearance {
                targets,
                appearance,
            } => format!("外観: {} を {appearance}", self.target_names(targets)),
        }
    }

    /// 動かす物の名前を並べた物(4 つ以上は「ほか」)。
    fn target_names(&self, targets: &[anim::Target]) -> String {
        let names: Vec<String> = targets.iter().map(|t| self.target_name(t)).collect();
        if names.len() > 3 {
            format!("{} ほか", names[..3].join("・"))
        } else {
            names.join("・")
        }
    }

    /// ジョイントの値の単位(回転は度、スライダは mm)。
    fn drive_unit(&self, joint: FeatureId) -> &'static str {
        match anim::drive_of(self.cur.work.report(), joint) {
            Some(d) if d.slide => " mm",
            _ => "°",
        }
    }

    /// 下のストーリーボード: 再生の操作と時刻の帯(段を長さの割合で並べる)、今の段の値と長さ。
    pub(crate) fn storyboard_ui(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        let steps = self.animation_steps_shown();
        let total = anim::duration(&steps);
        // 外観の段で選べる、このデザイン内で作った外観
        let own_looks: Vec<String> = kmb_look::appearance::of(self.cur.work.doc().data())
            .defs
            .into_iter()
            .map(|d| d.name)
            .collect();
        let mut editing = false;
        let mut new_steps: Option<(String, Vec<Step>)> = None;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let any = total > 0.0;
            if theme::icon_button(ui, "first", "初めへ", any, false)
                .on_hover_text("初めの時刻へ戻す(組み立てた形)")
                .clicked()
            {
                self.cur.anim.time = 0.0;
            }
            let playing = self.cur.anim.playing.is_some();
            let (icon, name, hint) = if playing {
                ("pause", "止める", "再生を止める")
            } else {
                (
                    "play",
                    "再生",
                    "段を初めから順に動かす(終わりにいれば初めから)",
                )
            };
            if theme::icon_button(ui, icon, name, any, playing)
                .on_hover_text(hint)
                .clicked()
            {
                self.toggle_animation_play();
            }
            if theme::icon_button(ui, "last", "終わりへ", any, false)
                .on_hover_text("終わりの時刻へ進める(全部の段を動かした形)")
                .clicked()
            {
                self.cur.anim.time = total;
            }
            ui.add_space(crate::timeline_ui::STRIP_GAP);
            // ビューの記録
            let rec = self.cur.anim.recording;
            let r = theme::icon_button(ui, "camera", "記録", self.cur.anim.auto.is_none(), rec)
                .on_hover_text(if rec {
                    "ビューを記録している: 視点を動かして止めるたびにビューの段を足す。押すと止める"
                } else {
                    "ビューを記録する: 入れている間、視点を動かして止めるたびにビューの段を足す"
                });
            if rec {
                // 記録中の印(赤い点)
                ui.painter().circle_filled(
                    r.rect.right_top() + Vec2::new(-4.0, 4.0),
                    3.5,
                    l.pal.err,
                );
            }
            if r.clicked() {
                self.set_view_recording(!rec);
            }
            ui.add_space(crate::timeline_ui::STRIP_GAP);
            ui.label(
                egui::RichText::new(format!("{:.2} / {:.2} 秒", self.cur.anim.time, total))
                    .color(l.pal.ink2),
            );
            ui.add_space(crate::timeline_ui::STRIP_GAP);
            // 時刻の帯: 段を長さの割合で並べ、今の時刻に縦の線。クリック・ドラッグで時刻を動かす。
            // 下に今の段の値の行があるので、枠はタイムラインの半分の高さ(地・縁・角は同じ)
            let w = ui.available_width().max(120.0);
            let (rect, r) = ui.allocate_exact_size(
                Vec2::new(w, crate::timeline_ui::strip_height() / 2.0),
                Sense::click_and_drag(),
            );
            crate::timeline_ui::paint_strip(ui, rect, &l);
            let p = ui.painter_at(rect);
            if total > 0.0 {
                let x_of = |t: f64| rect.left() + (t / total) as f32 * rect.width();
                let now = anim::step_at(&steps, self.cur.anim.time);
                let mut at = 0.0;
                for (i, s) in steps.iter().enumerate() {
                    let seg = egui::Rect::from_min_max(
                        egui::pos2(x_of(at) + 1.0, rect.top() + 3.0),
                        egui::pos2(x_of(at + s.seconds) - 1.0, rect.bottom() - 3.0),
                    );
                    let fill = if Some(i) == now {
                        l.pal.ws_soft(Space::Animation)
                    } else {
                        l.pal.paper
                    };
                    p.rect(
                        seg,
                        3.0,
                        fill,
                        Stroke::new(1.0, l.pal.ws(Space::Animation)),
                        egui::StrokeKind::Inside,
                    );
                    let short = match &s.kind {
                        StepKind::Explode { .. } | StepKind::ExplodeMany { .. } => {
                            format!("{} 分解", i + 1)
                        }
                        StepKind::Drive { .. } => format!("{} 動き", i + 1),
                        StepKind::View { .. } => format!("{} ビュー", i + 1),
                        StepKind::Visibility { visible, .. } => {
                            format!("{} {}", i + 1, if *visible { "表示" } else { "非表示" })
                        }
                        StepKind::Appearance { .. } => format!("{} 外観", i + 1),
                    };
                    let g = ui.fonts_mut(|f| f.layout_no_wrap(short, theme::prop(11.0), l.pal.ink));
                    if g.size().x + 6.0 < seg.width() {
                        p.galley(
                            egui::pos2(seg.left() + 4.0, seg.center().y - g.size().y / 2.0),
                            g,
                            l.pal.ink,
                        );
                    }
                    at += s.seconds;
                }
                let x = x_of(self.cur.anim.time);
                p.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(2.0, l.pal.ws(Space::Animation)),
                );
                if (r.clicked() || r.dragged())
                    && let Some(pos) = r.interact_pointer_pos()
                {
                    let f = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
                    self.cur.anim.time = f * total;
                    self.cur.anim.playing = None;
                }
            } else {
                p.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "段がありません。リボンの「分解の移動」「自動分解」「ジョイントの動き」「表示/非表示」「外観」「ビュー」で足します",
                    theme::prop(12.0),
                    l.pal.ink3,
                );
            }
        });
        // 今の時刻の段の値と長さ(自動分解の道具を出している間は直さない)
        if self.cur.anim.auto.is_some() {
            return;
        }
        if let Some(i) = anim::step_at(&steps, self.cur.anim.time) {
            let label = self.animation_step_label(&steps[i]);
            let unit = match &steps[i].kind {
                StepKind::Drive { joint, .. } => self.drive_unit(*joint),
                _ => " mm",
            };
            let view_now = self.view_pose();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("段 {}/{}: {label}", i + 1, steps.len()))
                        .color(l.pal.ink),
                );
                ui.add_space(12.0);
                let mut s = steps[i].clone();
                let mut changed = false;
                let track = |r: egui::Response, changed: &mut bool, editing: &mut bool| {
                    *changed |= r.changed();
                    *editing |= r.dragged() || r.has_focus();
                };
                match &mut s.kind {
                    StepKind::Explode { offset, .. } => {
                        let len = kmb_core::kernel::dot(*offset, *offset).sqrt();
                        let mut d = len;
                        ui.label("距離");
                        let r = ui.add(egui::DragValue::new(&mut d).speed(0.5).suffix(" mm"));
                        track(r, &mut changed, &mut editing);
                        if (d - len).abs() > 1e-12 && len > 0.0 && d.abs() > 1e-9 {
                            *offset = kmb_core::kernel::scale(*offset, d / len);
                        }
                    }
                    StepKind::ExplodeMany { moves } => {
                        // いちばん遠くへ動く物の距離。直すと全部の距離を同じ割合で変える
                        let len = moves
                            .iter()
                            .map(|m| kmb_core::kernel::dot(m.offset, m.offset).sqrt())
                            .fold(0.0, f64::max);
                        let mut d = len;
                        ui.label("いちばん遠い距離");
                        let r = ui.add(egui::DragValue::new(&mut d).speed(0.5).suffix(" mm"));
                        track(r, &mut changed, &mut editing);
                        if (d - len).abs() > 1e-12 && len > 0.0 && d > 1e-9 {
                            for m in moves.iter_mut() {
                                m.offset = kmb_core::kernel::scale(m.offset, d / len);
                            }
                        }
                    }
                    StepKind::Drive { from, to, .. } => {
                        ui.label("始め");
                        let r = ui.add(egui::DragValue::new(from).speed(1.0).suffix(unit));
                        track(r, &mut changed, &mut editing);
                        ui.label("終わり");
                        let r = ui.add(egui::DragValue::new(to).speed(1.0).suffix(unit));
                        track(r, &mut changed, &mut editing);
                    }
                    StepKind::View { view } => {
                        if ui
                            .button("今の視点にする")
                            .on_hover_text("この段の視点を、3D のビューの今の視点で置き換える")
                            .clicked()
                        {
                            *view = view_now;
                            changed = true;
                        }
                    }
                    StepKind::Visibility { visible, .. } => {
                        let r = ui
                            .checkbox(visible, "見せる")
                            .on_hover_text("入れると段の始めに見せ、切ると隠す");
                        changed |= r.changed();
                    }
                    StepKind::Appearance { appearance, .. } => {
                        ui.label("替える外観");
                        let before = appearance.clone();
                        // このデザイン内で作った外観を先に、その後にライブラリの外観
                        let own = &own_looks;
                        ui.menu_button(format!("{appearance} ▾"), |ui| {
                            let library = kmb_look::material::appearances()
                                .into_iter()
                                .map(|a| a.name.to_string())
                                .filter(|n| !own.contains(n));
                            for a in own.iter().cloned().chain(library) {
                                if ui.selectable_label(*appearance == a, &a).clicked() {
                                    *appearance = a;
                                    ui.close();
                                }
                            }
                        });
                        changed |= *appearance != before;
                    }
                }
                ui.add_space(8.0);
                ui.label("長さ");
                let r = ui.add(
                    egui::DragValue::new(&mut s.seconds)
                        .speed(0.05)
                        .range(0.1..=600.0)
                        .suffix(" 秒"),
                );
                track(r, &mut changed, &mut editing);
                if s.is_explode() {
                    ui.add_space(8.0);
                    let r = ui
                        .checkbox(&mut s.trail, "トレイルライン")
                        .on_hover_text("元の位置から今の位置への線を出す");
                    if r.changed() {
                        let mut list = steps.clone();
                        list[i] = s.clone();
                        new_steps = Some((
                            if s.trail {
                                "トレイルラインを出す".into()
                            } else {
                                "トレイルラインを消す".into()
                            },
                            list,
                        ));
                    }
                }
                if changed {
                    let mut list = steps.clone();
                    list[i] = s;
                    self.cur.anim.draft = Some(list);
                }
                ui.add_space(12.0);
                if theme::icon_button(ui, "back", "前へ", i > 0, false)
                    .on_hover_text("この段を 1 つ前へ")
                    .clicked()
                {
                    let mut list = steps.clone();
                    list.swap(i, i - 1);
                    new_steps = Some(("段を前へ".into(), list));
                }
                if theme::icon_button(ui, "fwd", "後ろへ", i + 1 < steps.len(), false)
                    .on_hover_text("この段を 1 つ後ろへ")
                    .clicked()
                {
                    let mut list = steps.clone();
                    list.swap(i, i + 1);
                    new_steps = Some(("段を後ろへ".into(), list));
                }
                if theme::icon_button(ui, "trash", "段を消す", true, false)
                    .on_hover_text("この段を消す")
                    .clicked()
                {
                    let mut list = steps.clone();
                    list.remove(i);
                    new_steps = Some(("段を消す".into(), list));
                }
            });
        }
        if let Some((label, list)) = new_steps {
            self.set_animation(&label, list);
        } else if !editing && let Some(d) = self.cur.anim.draft.take() {
            // 引くのを離した・打ち終えた: 文書に入れる(1 回の取り消し単位)
            self.set_animation("段を直す", d);
        }
    }
}

/// 向きの名前(軸に沿っていれば「Z」「-X」など、そうでなければ成分)。
fn direction_name(v: [f64; 3]) -> String {
    let n = kmb_core::kernel::dot(v, v).sqrt();
    if n < 1e-12 {
        return "動かない向き".into();
    }
    for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
        if (v[i].abs() / n - 1.0).abs() < 1e-9 {
            return if v[i] > 0.0 {
                (*a).to_string()
            } else {
                format!("-{a}")
            };
        }
    }
    format!("({}, {}, {})", num(v[0] / n), num(v[1] / n), num(v[2] / n))
}

/// 数を短く(整数なら小数なし、それ以外は小数 2 桁まで)。
fn num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// 分解の仕方のアイコンと名前。
fn mode_icon(sequential: bool) -> (&'static str, &'static str) {
    if sequential {
        ("sequential", "順次分解")
    } else {
        ("onestep", "ワンステップ分解")
    }
}
