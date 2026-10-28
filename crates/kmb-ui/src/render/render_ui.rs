//! レンダリングの作業空間の画面: キャンバス内レンダリング(今のビューを光線追跡で描き、止まって
//! いる間に少しずつきれいにする)と、表示の「効果」。
//!
//! 光線追跡は別のスレッドで回す。ビュー・ボディ・材質・環境が変わったら止めて、止まって少し
//! 待ってから描き直す(動かしている間はふつうの描き方のまま)。ビューの右下に進みの帯を出し、
//! 止める目安(回す数)の目盛・経過時間・回した数・一時停止を置く。

use crate::Workspace;
use crate::layout::Space;
use egui::{Rect, TextureHandle};
use kmb_ui_view3d::view3d;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 止める目安の目盛(回す数と名前)。0 は止めない。
pub(crate) const TARGETS: [(u32, &str); 4] = [
    (16, "下書き"),
    (64, "標準"),
    (256, "仕上げ"),
    (0, "止めない"),
];
/// 止める目安の既定(仕上げ)。
pub(crate) const DEFAULT_TARGET: u32 = 256;
/// 進みの帯の右端の回数(「止めない」の目盛の位置。帯は回数の対数で伸ばす)。
const BAR_END: u32 = 1024;
/// 絵の長い辺の上限(画素)。大きな画面でも 1 回を短くする。
const MAX_SIDE: f32 = 1280.0;
/// 動かすのを止めてから描き始めるまでの待ち。
const SETTLE: Duration = Duration::from_millis(250);

/// 描いている途中の絵と様子(別のスレッドが入れる)。
#[derive(Default)]
struct Progress {
    /// 新しい絵とその回した数(画面が取り出す)
    image: Option<(kmb_render::Image, u32)>,
    /// 回した数
    passes: u32,
    /// 回していた時間(一時停止と、目安に届いて待っている間は数えない)
    active: Duration,
}

type Shared = Arc<Mutex<Progress>>;

struct Job {
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    /// 止める目安の回数(0 は止めない)
    target: Arc<AtomicU32>,
    out: Shared,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// キャンバス内レンダリングの状態(押している間だけある)。
#[derive(Default)]
pub(crate) struct CanvasRender {
    /// 描いている物の鍵(ビュー・ボディ・材質・環境・大きさ)
    key: u64,
    changed_at: Option<Instant>,
    job: Option<Job>,
    texture: Option<TextureHandle>,
    /// 出している絵の回した数
    pub(crate) passes: u32,
    /// 回した数と回していた時間(進みの帯に出す)
    done: u32,
    active: Duration,
    /// 一時停止しているか
    pub(crate) paused: bool,
}

impl Workspace {
    /// レンダリングの「シーンの設定」を開く(設定の窓の「レンダリング」の頁)。
    pub(crate) fn open_render_settings(&mut self) {
        let id = self
            .win
            .extensions
            .settings()
            .into_iter()
            .find(|g| g.role == Some(kmb_interchange::role::settings::RENDER_SCENE))
            .map(|g| crate::settings_ui::module_page(&g.id, &g.group));
        match id {
            Some(id) => self.open_settings_at(&id),
            None => self.settings.open = true,
        }
    }

    /// キャンバス内レンダリングを始める・止める。
    pub(crate) fn toggle_canvas_render(&mut self) {
        if self.cur.canvas_render.take().is_none() {
            log::info!("キャンバス内レンダリングを始める");
            self.cur.canvas_render = Some(CanvasRender::default());
        } else {
            log::info!("キャンバス内レンダリングを止める");
        }
    }

    /// キャンバス内レンダリングをしているか。
    pub fn is_canvas_rendering(&self) -> bool {
        self.cur.canvas_render.is_some()
    }

    /// キャンバス内レンダリングで出している絵の回した数(していなければ `None`)。
    pub fn canvas_render_passes(&self) -> Option<u32> {
        self.cur.canvas_render.as_ref().map(|c| c.passes)
    }

    /// キャンバス内レンダリングの止める目安(回す数。0 は止めない)。
    pub fn canvas_render_target(&self) -> u32 {
        self.win.canvas_target
    }

    /// キャンバス内レンダリングの止める目安を変える(回す数。0 は止めない)。
    /// 届いて止まっている時に上げると、続きから回す。
    pub fn set_canvas_render_target(&mut self, target: u32) {
        log::info!(
            "キャンバス内レンダリングの止める目安: {}",
            if target == 0 {
                "止めない".to_string()
            } else {
                format!("{target} 回")
            }
        );
        self.win.canvas_target = target;
        if let Some(job) = self.cur.canvas_render.as_ref().and_then(|c| c.job.as_ref()) {
            job.target.store(target, Ordering::Relaxed);
        }
    }

    /// キャンバス内レンダリングを一時停止する・続ける。
    pub fn toggle_canvas_render_pause(&mut self) {
        if let Some(cr) = self.cur.canvas_render.as_mut() {
            cr.paused = !cr.paused;
            log::info!(
                "キャンバス内レンダリングを{}",
                if cr.paused {
                    "一時停止する"
                } else {
                    "続ける"
                }
            );
            if let Some(job) = &cr.job {
                job.pause.store(cr.paused, Ordering::Relaxed);
            }
        }
    }

    /// レンダリングのシーンの設定の、今の文書での値(JSON。モジュールが無ければ `None`)。
    fn render_scene_value(&self) -> Option<serde_json::Value> {
        self.cur
            .work
            .settings()
            .into_iter()
            .find(|g| g.role == Some(kmb_interchange::role::settings::RENDER_SCENE))
            .map(|g| g.value)
    }

    /// レンダリングのシーンの設定の環境(無ければ表示の「効果」の環境)。
    pub(crate) fn render_env(&self) -> kmb_render::EnvKind {
        self.render_scene_value()
            .and_then(|v| {
                v.get("env")
                    .and_then(|v| v.as_str())
                    .and_then(kmb_render::EnvKind::from_name)
            })
            .unwrap_or_else(|| self.win.layout.effects.env_kind())
    }

    /// レンダリングのシーンの設定の見え方(明るさ・背景・床・焦点距離・露出・被写界深度)。
    pub(crate) fn render_look(&self) -> kmb_render::SceneLook {
        let base = kmb_render::SceneLook::default();
        self.render_scene_value()
            .and_then(|v| kmb_render::SceneLook::from_json(&v, &base).ok())
            .unwrap_or(base)
    }

    /// ビュー `rect` の中の、静止画の縦横比の枠(レンダリングの作業空間)。
    /// 縦横比が「ビューポートの縦横比」ならビューそのもの。
    pub(crate) fn render_frame(&self, rect: Rect) -> Rect {
        if self.win.layout.space != Space::Render || rect.width() < 2.0 || rect.height() < 2.0 {
            return rect;
        }
        let v = self.render_scene_value().unwrap_or_default();
        let key = v.get("aspect").and_then(|a| a.as_str()).unwrap_or("custom");
        let int = |k: &str, d: u32| v.get(k).and_then(|x| x.as_u64()).map_or(d, |x| x as u32);
        let view = rect.width() / rect.height();
        let r = kmb_render::settings::aspect_ratio(
            key,
            int("width", 1600),
            int("height", 1000),
            Some(view),
        );
        // ビューに収まるいちばん大きな枠(少しの差は枠を出さない)
        if (r - view).abs() < 0.01 * view {
            return rect;
        }
        let (w, h) = if r > view {
            (rect.width(), rect.width() / r)
        } else {
            (rect.height() * r, rect.height())
        };
        Rect::from_center_size(rect.center(), egui::vec2(w, h))
    }

    /// 写実の絵の視点(ビュー `rect` の中の縦横比の枠を写す)。
    pub(crate) fn framed_render_camera(&self, rect: Rect) -> kmb_render::Camera {
        let frame = self.render_frame(rect);
        let cam = self.cur.camera.render_camera(rect);
        if frame == rect {
            return cam;
        }
        let px = |r: Rect| [r.center().x, r.center().y, r.width(), r.height()];
        cam.cropped(px(rect), px(frame))
    }

    /// 縦横比の枠の外を暗くする。
    fn render_frame_mask(&self, ui: &egui::Ui, rect: Rect) {
        let frame = self.render_frame(rect);
        if frame == rect {
            return;
        }
        let shade = egui::Color32::from_black_alpha(150);
        let p = ui.painter_at(rect);
        for r in [
            Rect::from_min_max(rect.min, egui::pos2(rect.right(), frame.top())),
            Rect::from_min_max(egui::pos2(rect.left(), frame.bottom()), rect.max),
            Rect::from_min_max(
                egui::pos2(rect.left(), frame.top()),
                egui::pos2(frame.left(), frame.bottom()),
            ),
            Rect::from_min_max(
                egui::pos2(frame.right(), frame.top()),
                egui::pos2(rect.right(), frame.bottom()),
            ),
        ] {
            if r.width() > 0.0 && r.height() > 0.0 {
                p.rect_filled(r, 0.0, shade);
            }
        }
        p.rect_stroke(
            frame,
            0.0,
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(120)),
            egui::StrokeKind::Outside,
        );
    }

    /// 3D のビューへ写実の見た目を渡す(表示の「効果」。レンダリングの作業空間では全部のボディを
    /// 材質で描き、環境はシーンの設定の物)。毎フレーム呼ぶ。
    pub(crate) fn sync_effects(&mut self) {
        let render = self.win.layout.space == Space::Render;
        // レンダリングの作業空間の透視投影は、シーンの設定の焦点距離の画角
        self.cur.camera.fov_deg = if render {
            self.render_look().fov_y_deg()
        } else {
            kmb_ui_view3d::view3d::FOV_DEG
        };
        self.cur.scene.effects = kmb_ui_view3d::Effects {
            materials: self.win.layout.effects.materials || render,
            all_bodies: render,
            env: if render {
                self.render_env()
            } else {
                self.win.layout.effects.env_kind()
            },
        };
    }

    /// 3D のビューの上にキャンバス内レンダリングの絵を重ねる(レンダリングの作業空間で押している時)。
    pub(crate) fn canvas_render_ui(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        colors: &view3d::Colors,
    ) {
        if self.win.layout.space != Space::Render {
            self.cur.canvas_render = None;
            return;
        }
        self.render_frame_mask(ui, rect);
        // 絵は縦横比の枠の中に描く
        let view = rect;
        let rect = self.render_frame(view);
        if self.cur.canvas_render.is_none() || rect.width() < 8.0 || rect.height() < 8.0 {
            return;
        }
        let ppp = ui.ctx().pixels_per_point();
        let scale = (MAX_SIDE / (rect.width().max(rect.height()) * ppp)).min(1.0) * ppp;
        let size = [
            (rect.width() * scale).round().max(1.0) as u32,
            (rect.height() * scale).round().max(1.0) as u32,
        ];
        let look = self.render_look();
        let mut cam = self.framed_render_camera(view);
        look.apply_camera(&mut cam);
        let env_kind = self.cur.scene.effects.env;
        let key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            format!("{cam:?}").hash(&mut h);
            format!("{look:?}").hash(&mut h);
            size.hash(&mut h);
            self.cur.scene.generation().hash(&mut h);
            env_kind.hash(&mut h);
            for i in 0..self.cur.scene.bodies.len() {
                format!("{:?}", self.cur.scene.body_look(i, colors)).hash(&mut h);
                self.cur.scene.body_color(i, colors).hash(&mut h);
            }
            h.finish()
        };
        let now = Instant::now();
        let ctx = ui.ctx().clone();
        let target = self.win.canvas_target;
        let Some(cr) = self.cur.canvas_render.as_mut() else {
            return;
        };
        if cr.key != key {
            // 変わった: 止めて、落ち着くのを待つ
            cr.key = key;
            cr.job = None;
            cr.texture = None;
            cr.passes = 0;
            cr.done = 0;
            cr.active = Duration::ZERO;
            cr.changed_at = Some(now);
        }
        if cr.job.is_none() {
            let since = cr.changed_at.map_or(SETTLE, |t| now - t);
            if since < SETTLE {
                ctx.request_repaint_after(SETTLE - since);
            } else {
                let scene = self.cur.scene.render_scene(colors);
                let env = look.env(env_kind, cam.toward_eye());
                let job = start(scene, cam, env, &look, size, ctx.clone());
                job.target.store(target, Ordering::Relaxed);
                job.pause.store(cr.paused, Ordering::Relaxed);
                cr.job = Some(job);
            }
        }
        // 新しい絵が来ていれば絵を替える
        let fresh = cr.job.as_ref().and_then(|job| {
            let mut o = job.out.lock().unwrap_or_else(|e| e.into_inner());
            cr.done = o.passes;
            cr.active = o.active;
            o.image.take()
        });
        if let Some((img, passes)) = fresh {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [img.width as usize, img.height as usize],
                &img.rgba,
            );
            match &mut cr.texture {
                Some(t) => t.set(image, egui::TextureOptions::LINEAR),
                None => {
                    cr.texture = Some(ctx.load_texture(
                        "kmb-canvas-render",
                        image,
                        egui::TextureOptions::LINEAR,
                    ))
                }
            }
            cr.passes = passes;
        }
        if let Some(t) = &cr.texture {
            ui.painter_at(rect).image(
                t.id(),
                rect,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        self.canvas_render_bar(ui, view);
    }

    /// キャンバス内レンダリングの進みの帯(ビューの右下): 一時停止、止める目安の目盛つきの帯、
    /// 経過時間と回した数。目盛の名前を押すと、その回数で止める。
    fn canvas_render_bar(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let Some(cr) = self.cur.canvas_render.as_ref() else {
            return;
        };
        let l = self.win.look;
        let (done, active, paused, ready) = (cr.done, cr.active, cr.paused, cr.texture.is_some());
        let target = self.win.canvas_target;
        let reached = target > 0 && done >= target;
        let w = 300.0_f32.min(rect.width() - 24.0);
        let h = 58.0;
        if w < 160.0 {
            return;
        }
        let br = Rect::from_min_size(
            egui::pos2(
                rect.right() - 12.0 - w,
                rect.bottom() - 12.0 - 32.0 - 8.0 - h,
            ),
            egui::vec2(w, h),
        );
        let mut pick = None;
        let mut toggle = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(br), |ui| {
            egui::Frame::new()
                .fill(l.pal.panel)
                .stroke(egui::Stroke::new(1.0, l.pal.edge))
                .corner_radius(crate::theme::corner(6.0))
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.set_width(w - 12.0);
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
                    ui.horizontal(|ui| {
                        let (icon, name) = if paused {
                            ("play", "続ける")
                        } else {
                            ("pause", "一時停止")
                        };
                        if crate::theme::icon_button(ui, icon, name, !reached, paused)
                            .on_hover_text(format!("キャンバス内レンダリングを{name}"))
                            .clicked()
                        {
                            toggle = true;
                        }
                        // 帯と目盛(目盛の名前は帯の上、押すとその回数で止める)
                        let (r, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 30.0),
                            egui::Sense::hover(),
                        );
                        let bar = Rect::from_min_max(
                            egui::pos2(r.left() + 4.0, r.bottom() - 9.0),
                            egui::pos2(r.right() - 22.0, r.bottom() - 4.0),
                        );
                        let at = |n: u32| {
                            let k = (n.max(1) as f32).log2() / (BAR_END as f32).log2();
                            bar.left() + bar.width() * k.clamp(0.0, 1.0)
                        };
                        let p = ui.painter();
                        p.rect_filled(bar, 2.0, l.pal.edge);
                        if done > 0 {
                            let mut fill = bar;
                            fill.set_right(at(done));
                            p.rect_filled(fill, 2.0, l.ws());
                        }
                        for (n, name) in TARGETS {
                            let x = if n == 0 { bar.right() } else { at(n) };
                            let on = n == target;
                            let g = ui.fonts_mut(|f| {
                                f.layout_no_wrap(
                                    name.to_string(),
                                    crate::theme::prop(10.5),
                                    if on { l.ws() } else { l.pal.ink2 },
                                )
                            });
                            let tr = Rect::from_center_size(
                                egui::pos2(x, r.top() + 2.0 + g.size().y / 2.0),
                                g.size() + egui::vec2(6.0, 2.0),
                            );
                            let tr =
                                tr.translate(egui::vec2((r.right() - tr.right()).min(0.0), 0.0));
                            let resp = ui
                                .interact(
                                    tr,
                                    ui.id().with(("canvas_target", n)),
                                    egui::Sense::click(),
                                )
                                .on_hover_text(if n == 0 {
                                    "止めずに回し続ける(一時停止で止める)".to_string()
                                } else {
                                    format!("{n} 回で止める")
                                });
                            if resp.clicked() {
                                pick = Some(n);
                            }
                            let p = ui.painter();
                            if on || resp.hovered() {
                                p.rect_stroke(
                                    tr,
                                    3.0,
                                    egui::Stroke::new(1.0, if on { l.ws() } else { l.pal.edge }),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            p.galley(tr.min + egui::vec2(3.0, 1.0), g, l.pal.ink);
                            p.line_segment(
                                [
                                    egui::pos2(x, bar.top() - 3.0),
                                    egui::pos2(x, bar.bottom() + 1.0),
                                ],
                                egui::Stroke::new(1.0, if on { l.ws() } else { l.pal.ink3 }),
                            );
                        }
                    });
                    let state = if !ready {
                        "準備中…"
                    } else if paused {
                        "一時停止"
                    } else if reached {
                        "完了"
                    } else {
                        "描いています"
                    };
                    ui.horizontal(|ui| {
                        let small = |t: String| {
                            egui::RichText::new(t)
                                .size(crate::theme::fs(11.0))
                                .color(l.pal.ink2)
                        };
                        ui.label(small(format!(
                            "{state} · 経過 {}",
                            crate::frame_ui::clock(active.as_secs_f64())
                        )));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // 目安を回した数より下げた時は、目安を書かない
                            ui.label(small(if target > 0 && done <= target {
                                format!("回数 {done}/{target}")
                            } else {
                                format!("回数 {done}")
                            }));
                        });
                    });
                });
        });
        if toggle {
            self.toggle_canvas_render_pause();
        }
        if let Some(n) = pick {
            self.set_canvas_render_target(n);
        }
    }

    /// 表示の「効果」(材質の質感と環境の光)。表示のメニューと設定の窓で使う。
    pub(crate) fn effects_items(&mut self, ui: &mut egui::Ui) {
        let (name, tip) = (
            self.win.t("view-materials"),
            self.win.t("view-materials-tip"),
        );
        crate::keycast::command_response(
            ui.checkbox(&mut self.win.layout.effects.materials, &name),
            name,
        )
        .on_hover_text(tip);
        // 光の名前は鍵(`studio_dark`)の `_` を `-` にした ID で引く
        let envs: Vec<(String, String)> = kmb_render::EnvKind::ALL
            .iter()
            .map(|e| {
                let id = format!("view-env-{}", e.key().replace('_', "-"));
                (e.key().to_string(), self.win.t(&id))
            })
            .collect();
        ui.add_enabled_ui(self.win.layout.effects.materials, |ui| {
            for (key, name) in envs {
                crate::keycast::command_response(
                    ui.radio_value(&mut self.win.layout.effects.env, key, &name),
                    name,
                );
            }
        });
    }
}

/// 別のスレッドで光線追跡を回し始める。止める目安に届くか一時停止の間は、絵を出して待つ。
fn start(
    scene: kmb_render::RenderScene,
    cam: kmb_render::Camera,
    env: kmb_render::Env,
    look: &kmb_render::SceneLook,
    size: [u32; 2],
    ctx: egui::Context,
) -> Job {
    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    let target = Arc::new(AtomicU32::new(DEFAULT_TARGET));
    let out: Shared = Arc::new(Mutex::new(Progress::default()));
    let (s, ps, tg, o) = (stop.clone(), pause.clone(), target.clone(), out.clone());
    // 画面の上に重ねるので、透明の背景も環境で描く(単色はそのまま)
    let background = match look.background {
        kmb_render::Background::Transparent => kmb_render::Background::Studio,
        b => b,
    };
    let ground = look.ground;
    let spawned = std::thread::Builder::new()
        .name("kmb-canvas-render".into())
        .spawn(move || {
            let opt = kmb_render::trace::TraceOptions {
                width: size[0],
                height: size[1],
                background,
                ground,
                ..Default::default()
            };
            let mut tracer = kmb_render::trace::Tracer::new(&scene, cam, env, opt);
            let mut show = 1;
            let mut shown = 0;
            let mut active = Duration::ZERO;
            let put = |tracer: &kmb_render::trace::Tracer| {
                let img = tracer.image(true);
                o.lock().unwrap_or_else(|e| e.into_inner()).image = Some((img, tracer.passes));
                ctx.request_repaint();
            };
            while !s.load(Ordering::Relaxed) {
                let t = tg.load(Ordering::Relaxed);
                let reached = t > 0 && tracer.passes >= t;
                if reached || ps.load(Ordering::Relaxed) {
                    // 待つ前に、ここまでの絵を出す
                    if shown < tracer.passes {
                        shown = tracer.passes;
                        put(&tracer);
                        log::info!(
                            "キャンバス内レンダリング: {}×{} を {} 回({:.1} 秒){}",
                            size[0],
                            size[1],
                            tracer.passes,
                            active.as_secs_f64(),
                            if reached {
                                "。目安に届いた"
                            } else {
                                "。一時停止"
                            }
                        );
                    }
                    std::thread::sleep(Duration::from_millis(30));
                    continue;
                }
                let t0 = Instant::now();
                tracer.pass();
                active += t0.elapsed();
                {
                    let mut g = o.lock().unwrap_or_else(|e| e.into_inner());
                    g.passes = tracer.passes;
                    g.active = active;
                }
                if s.load(Ordering::Relaxed) {
                    break;
                }
                if tracer.passes >= show || (t > 0 && tracer.passes >= t) {
                    shown = tracer.passes;
                    put(&tracer);
                    // 1・2・4・8・16 回と、その後は 16 回ごとに見せる
                    while show <= tracer.passes {
                        show = (show * 2).min(show + 16);
                    }
                } else {
                    // 経過と回数を出し直す
                    ctx.request_repaint();
                }
            }
        });
    if let Err(e) = spawned {
        log::warn!("キャンバス内レンダリングのスレッドを作れません: {e}");
    }
    Job {
        stop,
        pause,
        target,
        out,
    }
}
