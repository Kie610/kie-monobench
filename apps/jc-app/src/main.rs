//! GUI の実行ファイル。
//!
//! `kie-monobench` で開始画面から始める。`kie-monobench <ファイル>` で文書を開く(なければ新規に作る)。
//! `kie-monobench --sample <ファイル>` で、基準部品(穴あきの板)とその加工から始める。
//! `kie-monobench --safe-mode` で安全モード(既定の設定・ソフトウェア描画。`JC_SAFE_MODE=1` も同じ)。
//! `JC_RENDERER=cpu` はソフトウェア描画だけを選ぶ(設定はそのまま)。`JC_RENDERER=dxc|fxc|vulkan` は
//! 描画の API を、`JC_RENDERER=auto` は wgpu の既定の選び方(全部の API を探す)を、設定に代えて選ぶ。
//!
//! 起動の安全策(jc_session::startup): 読めない設定ファイルは脇へ退けて既定で始める。窓が開く前に
//! 2 回続けて止まったら、次は安全モードで始める。設定で選んだ描画の API で窓を開けなければ、
//! wgpu の既定の選び方で、それもだめならソフトウェア描画で開き直し、それもだめなら何が起きたかを
//! 知らせる(黙って閉じない)。
//!
//! Windows のリリース版は窓のアプリとして作り、起動の時に黒い窓(コンソール)を出さない(N7a)。
//! 記録はいつも通りログ(`%APPDATA%\kie-monobench\logs`、`JC_LOG` でレベル)に出る。端末から引数を
//! 付けて起動してもよいが、標準エラーは端末に出ない(要れば `| Out-Host` で待って受ける。docs/verify.md)。
//! 窓が開く前に止まる誤りは、ログに加えて OS の窓で知らせる。デバッグ版は端末に出す。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod surface;
#[cfg(windows)]
mod win_start;

use eframe::egui;
use jc_core::Document;
use jc_session::startup;
use jc_ui::view3d_render::Api;
use jc_ui::{Dialogs, Unsaved, Workspace, install_system_fonts};
use std::path::PathBuf;

/// Linux で OS のファイルの窓を出せない理由。rfd は xdg-desktop-portal(D-Bus)か zenity で窓を出すので、
/// D-Bus のセッションも zenity も無ければ出せない(Z24)。ほかの OS は常に出せるとみなす。
fn dialog_unavailable() -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let dbus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some_and(|v| !v.is_empty());
    let zenity = std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("zenity").is_file()));
    (!dbus && !zenity).then(|| {
        "D-Bus のセッションも zenity も見つかりません。xdg-desktop-portal か zenity を入れてください".into()
    })
}

/// 開き直した子(既定の選び方かソフトウェア描画)に、前の誤りを渡す環境変数。
const RETRY_ENV: &str = "JC_RENDERER_RETRY";

/// OS のファイルダイアログ(rfd)。
struct OsDialogs;

impl Dialogs for OsDialogs {
    fn unavailable(&self) -> Option<String> {
        dialog_unavailable()
    }

    fn pick_folder(&self, title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_folder()
    }

    fn open_document(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("文書を開く")
            .add_filter("kieMonoBench の文書", jc_core::DOCUMENT_EXTENSIONS)
            .pick_file()
    }

    fn save_document(&self, name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("名前を付けて保存")
            .add_filter("kieMonoBench の文書", jc_core::DOCUMENT_EXTENSIONS)
            .set_file_name(name)
            .save_file()
    }

    fn open_file(&self, title: &str, filter: &str, extensions: &[&str]) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter(filter, extensions)
            .pick_file()
    }

    fn save_file(
        &self,
        title: &str,
        filter: &str,
        extensions: &[&str],
        name: &str,
    ) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter(filter, extensions)
            .set_file_name(name)
            .save_file()
    }

    fn unsaved(&self, name: &str) -> Unsaved {
        let r = rfd::MessageDialog::new()
            .set_title("保存していない変更")
            .set_description(format!("「{name}」の変更を保存しますか?"))
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                "保存する".into(),
                "保存しない".into(),
                "キャンセル".into(),
            ))
            .show();
        match r {
            rfd::MessageDialogResult::Custom(s) if s == "保存する" => Unsaved::Save,
            rfd::MessageDialogResult::Custom(s) if s == "保存しない" => Unsaved::Discard,
            rfd::MessageDialogResult::Yes => Unsaved::Save,
            rfd::MessageDialogResult::No => Unsaved::Discard,
            _ => Unsaved::Cancel,
        }
    }
}

/// 仮のアイコン(apps/jc-app/assets/make_icon.py で作る)。exe の分は build.rs で埋め込む
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

/// 起動の区切りごとの時間をログに書く(N7c。PC の実機で起動のどこが遅いかを見る)。
struct Startup {
    began: std::time::Instant,
    last: std::time::Instant,
}

impl Startup {
    fn new() -> Startup {
        let now = std::time::Instant::now();
        Startup {
            began: now,
            last: now,
        }
    }

    fn mark(&mut self, what: &str) {
        let now = std::time::Instant::now();
        log::info!(
            "起動の区切り: {what}(この区切り {:.0} ms、開始から {:.0} ms)",
            (now - self.last).as_secs_f64() * 1e3,
            (now - self.began).as_secs_f64() * 1e3
        );
        self.last = now;
    }
}

struct App {
    /// 外へ出した部品の窓も中身を描けるよう、共有の形で持つ(`Workspace::into_shared`)
    workspace: std::sync::Arc<std::sync::Mutex<Workspace>>,
    /// 最初の画面を描いたら起動の記録を戻すデータフォルダ
    starting: Option<PathBuf>,
    /// 起動の区切りの時間(最初の画面と最初の再計算が終わるまで)
    timing: Option<(Startup, u8)>,
    /// 画面の速さを測っている時(`JC_BENCH_VIEW`)
    bench: Option<Bench>,
    /// 描いた画面の数(2 まで。最初の画面を出した後で起動の窓の覆いを外す。Windows。win_start)
    frames: u8,
    /// 窓が最小化されているか(最後に描いた画面での値)
    minimized: bool,
    /// 最小化する前の窓の位置と大きさ(最小化を解除すると戻る状態)
    placement: Option<Placement>,
}

/// eframe が窓の位置と大きさを保存する鍵
const WINDOW_KEY: &str = "window";

/// 最小化していない時の窓の位置と大きさ(最後に描いた画面での値)
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    outer: egui::Rect,
    inner: egui::Rect,
    /// 点 1 つの画素数(表示倍率 × 拡大率)
    ppp: f32,
    maximized: bool,
    fullscreen: bool,
}

impl Placement {
    /// 最小化中は位置が無いので None
    fn of(ctx: &egui::Context) -> Option<Self> {
        ctx.input(|i| {
            let v = i.viewport();
            Some(Self {
                outer: v.outer_rect?,
                inner: v.inner_rect?,
                ppp: i.pixels_per_point,
                maximized: v.maximized.unwrap_or(false),
                fullscreen: v.fullscreen.unwrap_or(false),
            })
        })
    }

    /// eframe が "window" に書くのと同じ形(egui_winit::WindowSettings の ron。位置は画素、大きさは点)
    fn ron(&self) -> String {
        let (o, i, s) = (
            self.outer.min * self.ppp,
            self.inner.min * self.ppp,
            self.inner.size(),
        );
        format!(
            "(inner_position_pixels:Some((x:{:?},y:{:?})),outer_position_pixels:Some((x:{:?},y:{:?})),fullscreen:{},maximized:{},inner_size_points:Some((x:{:?},y:{:?})))",
            i.x.round(),
            i.y.round(),
            o.x.round(),
            o.y.round(),
            self.fullscreen,
            self.maximized,
            s.x,
            s.y
        )
    }
}

/// 画面の速さを測る: 等角で回す・上から見て動かすを、それぞれ決めたフレーム数だけ続け、
/// フレームの間隔をログに書いて閉じる(垂直同期なし)。
struct Bench {
    frames: u32,
    frame: u32,
    last: Option<std::time::Instant>,
    times: Vec<f64>,
    /// フレームごとの、3D のビューを描く・拾うのにかかった時間(ms)
    parts: Vec<(f64, f64)>,
}

impl Bench {
    const ENV: &str = "JC_BENCH_VIEW";
    /// 測り始める前に待つフレーム(開いた直後の網作りなどを除く)
    const WARMUP: u32 = 30;

    fn from_env() -> Option<Bench> {
        let frames = std::env::var(Self::ENV).ok()?.parse::<u32>().ok()?.max(10);
        Some(Bench {
            frames,
            frame: 0,
            last: None,
            times: Vec::new(),
            parts: Vec::new(),
        })
    }

    /// 1 フレーム進める。視点を動かし、測り終えたら真。
    fn step(&mut self, ws: &mut Workspace) -> bool {
        let now = std::time::Instant::now();
        // 開いた文書の再計算が終わるまでは数え始めない(形の無い画面を測ってしまう)
        if self.frame == 0 && ws.is_busy() {
            return false;
        }
        let n = self.frames;
        let phase = |f: u32| (f.saturating_sub(Self::WARMUP)) / n;
        if let Some(t) = self.last
            && self.frame > Self::WARMUP
            && phase(self.frame - 1) == phase(self.frame)
        {
            self.times.push((now - t).as_secs_f64() * 1000.0);
            if let Some(f) = ws.last_frame() {
                self.parts.push((f.draw, f.pick));
            }
        }
        self.last = Some(now);
        let p = phase(self.frame);
        if self.frame >= Self::WARMUP && (self.frame - Self::WARMUP) % n == n - 1 {
            self.report(if p == 0 {
                "等角で回す"
            } else {
                "上から見て動かす"
            });
        }
        if p >= 2 {
            return true;
        }
        ws.nudge_view(p == 1, self.frame);
        self.frame += 1;
        false
    }

    fn report(&mut self, what: &str) {
        let mut t = std::mem::take(&mut self.times);
        if t.is_empty() {
            return;
        }
        t.sort_by(f64::total_cmp);
        let mean = t.iter().sum::<f64>() / t.len() as f64;
        let parts = std::mem::take(&mut self.parts);
        let n = parts.len().max(1) as f64;
        let (draw, pick) = parts
            .iter()
            .fold((0.0, 0.0), |a, p| (a.0 + p.0 / n, a.1 + p.1 / n));
        let line = format!(
            "画面の速さ({what}、{} フレーム): 平均 {:.1} fps(1 フレーム {mean:.1} ms、中央値 {:.1} ms、遅い方から 5% {:.1} ms。うち 3D を描く {draw:.2} ms、拾う {pick:.3} ms)",
            t.len(),
            1000.0 / mean,
            t[t.len() / 2],
            t[(t.len() * 95 / 100).min(t.len() - 1)]
        );
        log::info!("{line}");
        eprintln!("{line}");
    }
}

impl eframe::App for App {
    // 最小化中の eframe は ui を呼ばず logic だけを呼ぶので、最小化はここで見る
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.minimized = ctx.input(|i| i.viewport().minimized == Some(true));
        if !self.minimized {
            self.placement = Placement::of(ctx).or(self.placement);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let shared = self.workspace.clone();
        let mut workspace = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let workspace = &mut *workspace;
        if let Some(dir) = self.starting.take() {
            jc_session::startup::started(&dir);
        }
        // eframe は最初の画面を描いた後で窓を出す。次の画面で覆いを外すので、すぐ描き直させる
        match self.frames {
            0 => ui.ctx().request_repaint(),
            1 => {
                #[cfg(windows)]
                win_start::after_first_frame();
            }
            _ => {}
        }
        self.frames = self.frames.saturating_add(1).min(2);
        if let Some((t, stage)) = &mut self.timing {
            if *stage == 0 {
                t.mark("最初の画面を作り始める");
                *stage = 1;
            } else if *stage == 1 {
                t.mark("最初の画面を描き終える");
                *stage = 2;
            }
            if *stage == 2 && !workspace.is_busy() {
                t.mark("開いた文書の再計算が終わる");
                self.timing = None;
            }
        }
        if let Some(b) = &mut self.bench {
            if b.step(workspace) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ui.ctx().request_repaint();
        }
        // 窓を閉じる前に、未保存の変更を確かめる
        if ui.ctx().input(|i| i.viewport().close_requested()) && !workspace.confirm_close() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        let started = std::time::Instant::now();
        workspace.ui(ui);
        log::trace!(
            "1 フレームの画面作り: {:.2} ms",
            started.elapsed().as_secs_f64() * 1000.0
        );
        // クラッシュ報告に、最近の操作と文書を残す(変わった時だけ書く)
        jc_session::crash::note(workspace.document());
    }

    // eframe は最小化中も窓の位置(Windows では -32000)と極小の大きさをそのまま覚え、
    // 次の起動が左上の極小の窓になる(eframe 0.36)。窓の値を書いた後でここが呼ばれるので、
    // 最小化中は、最小化を解除すると戻る状態(最小化する前の位置・大きさ・最大化)に書き換える。
    // 最小化のまま起動して一度も戻さなかった時は消して、初めての大きさで開く
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if !self.minimized {
            return;
        }
        match self.placement {
            Some(p) => storage.set_string(WINDOW_KEY, p.ron()),
            None => storage.remove_string(WINDOW_KEY),
        }
    }
}

fn main() -> Result<(), String> {
    let mut timing = Startup::new();
    // ログとクラッシュ報告を手元に残す(送信しない)。残せなくても動く
    jc_session::logging::init(jc_session::default_data_dir().as_deref(), "kie-monobench");
    timing.mark("ログを始める");
    log::info!("引数: {:?}", std::env::args().skip(1).collect::<Vec<_>>());
    let _crash = jc_session::default_data_dir().and_then(|d| {
        jc_session::crash::install(&d, "kie-monobench")
            .map_err(|e| eprintln!("クラッシュ報告を残せません: {e}"))
            .ok()
    });
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut safe_mode = startup::env_on(std::env::var(startup::SAFE_MODE_ENV).ok().as_deref());
    if let Some(i) = args.iter().position(|a| a == "--safe-mode") {
        args.remove(i);
        safe_mode = true;
    }
    // 読めない設定は退け、窓が開く前に止まった起動を数える
    let data_dir = jc_session::default_data_dir();
    let mut notes: Vec<String> = Vec::new();
    if let Some(dir) = &data_dir {
        notes.extend(startup::set_aside_broken(dir));
        let failed = startup::begin(dir);
        if failed >= startup::FAILED_STARTS_FOR_SAFE_MODE && !safe_mode {
            safe_mode = true;
            notes.push(format!(
                "前の起動が {failed} 回続けて、窓が開く前に止まったので、安全モードで始めました"
            ));
        }
    }
    // 設定のうち、窓を開く前に決まる物(描画装置・垂直同期。U26。窓の帯。N4)。安全モードは既定のまま
    let (render, system_title_bar) = match (&data_dir, safe_mode) {
        (Some(dir), false) => {
            let l = jc_ui::Layout::load(dir);
            (l.render, l.system_title_bar)
        }
        // 安全モードは Windows の標準の帯(アプリが描く帯で困った時の逃げ道)
        (_, true) => (jc_ui::RenderSettings::default(), true),
        _ => (jc_ui::RenderSettings::default(), false),
    };
    let renderer_env = std::env::var(startup::RENDERER_ENV).ok();
    let cpu = safe_mode
        || render.device == jc_ui::view3d_render::Device::Software
        || startup::cpu_renderer_env(renderer_env.as_deref());
    // 描画の API: 環境変数が勝ち、なければ設定。ソフトウェア描画では使わない(None は wgpu の既定の選び方)
    let api = match renderer_env.as_deref() {
        _ if cpu => None,
        Some(v) if v.trim().eq_ignore_ascii_case("auto") => None,
        Some(v) if Api::from_key(v).is_some() => Api::from_key(v),
        _ => Some(render.api),
    };
    // GPU で窓を開けず、開き直した時は、前の誤りを知らせる
    if let Ok(e) = std::env::var(RETRY_ENV) {
        notes.push(if cpu {
            format!(
                "GPU で画面を開けなかったので、ソフトウェア描画で開き直しました(遅くなります): {e}"
            )
        } else {
            format!(
                "設定で選んだ描画の API で画面を開けなかったので、自動で選び直しました\
                 (設定の「描画」で替えられます): {e}"
            )
        });
    }
    if safe_mode {
        notes.push(
            "安全モード: 既定の設定と画面の配置で、ソフトウェア描画で動いています。\
             設定は書き換えません(次の普通の起動で元に戻ります)"
                .into(),
        );
    }
    log::info!(
        "安全モード: {safe_mode}、ソフトウェア描画: {cpu}、描画装置: {:?}、描画の API: {}、垂直同期: {}",
        render.device,
        api.map_or("自動", Api::label),
        render.vsync
    );
    let sample = args.first().is_some_and(|a| a == "--sample");
    if sample {
        args.remove(0);
    }
    let path = args.first().map(PathBuf::from);
    let (doc, start) = match &path {
        _ if sample => (jc_bundle::Sample::Cam.make()(), false),
        Some(p) if p.exists() => match jc_session::load(p) {
            Ok(doc) => (doc, false),
            Err(e) => {
                return fail("文書を開けません", &format!("{}\n\n{e}", p.display()));
            }
        },
        Some(_) => (Document::new(), false),
        None => (Document::new(), true),
    };
    timing.mark("設定を確かめ、文書を読む");
    // 専用の編集器(汎用の欄では表せない種類)はここで組み込む
    let mut workspace = Workspace::open(doc, path, true, jc_bundle::modules())
        // 文書のタブごとに形のカーネルを作る(分け合うと片方の計算でもう片方の形が消える)
        .with_kernel_factory(std::sync::Arc::new(jc_bundle::kernel))
        .with_editor(jc_ui_sketch::SketchEditor::new())
        .with_editor(jc_ui_sketch::Sketch3dEditor::new())
        .with_editor(jc_ui_tools::ToolsEditor::new())
        .with_editor(jc_ui_text::TextEditor::new())
        .with_home(jc_ui_library::Library::new())
        .with_dialogs(Box::new(OsDialogs))
        .with_samples(
            jc_bundle::Sample::ALL
                .iter()
                .map(|s| (s.label().to_string(), s.make()))
                .collect(),
        );
    if safe_mode {
        workspace = workspace.with_safe_mode();
    }
    if let Some(dir) = data_dir.clone() {
        workspace = workspace.with_data_dir(dir);
    }
    if start {
        workspace = workspace.show_start();
    }
    workspace = workspace.with_notice(&notes);
    timing.mark("作業の場を組み立てる");
    let title = if safe_mode {
        "kieMonoBench(安全モード)"
    } else {
        "kieMonoBench"
    };
    let bench = Bench::from_env();
    let (wgpu_options, compiler) =
        wgpu_options(cpu, bench.is_some() || !render.vsync, render.device, api);
    let samples = render.samples();
    let options = eframe::NativeOptions {
        // 窓の位置・大きさと表示倍率を覚える(読めなければ既定で開く)。
        // 安全モードは覚えたものを使わず、一時ファイルに書く(利用者の分は残す)
        persist_window: !safe_mode,
        persistence_path: safe_mode.then(|| {
            let scratch =
                std::env::temp_dir().join(format!("kie-monobench-safe-{}.ron", std::process::id()));
            let _ = std::fs::remove_file(&scratch);
            scratch
        }),
        wgpu_options,
        // 初めて開く時の大きさ(試作の画面と同じ 1280×760)
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([1280.0, 760.0])
            .with_min_inner_size([960.0, 600.0])
            // 窓の帯はアプリが描く(上の帯の右端に最小化・最大化・閉じる。N4)
            .with_decorations(system_title_bar)
            .with_icon(icon().unwrap_or_default()),
        // 覚えた位置・大きさ・最大化を当てた後の、窓の作り方を書く(起動時の窓の見え方を調べる手掛かり)
        window_builder: Some(Box::new(|b| {
            log::info!(
                "窓の作り方: 位置 {:?}、中の大きさ {:?}、最小 {:?}、最大化 {:?}、全画面 {:?}",
                b.position,
                b.inner_size,
                b.min_inner_size,
                b.maximized,
                b.fullscreen
            );
            b
        })),
        ..Default::default()
    };
    // 描く前の窓が一瞬出ないよう、作られた窓を最初の画面まで覆う(Windows。win_start)
    #[cfg(windows)]
    win_start::before_window();
    let created = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let created_in_app = created.clone();
    let result = eframe::run_native(
        "kie-monobench",
        options,
        Box::new(move |cc| {
            created_in_app.store(true, std::sync::atomic::Ordering::SeqCst);
            timing.mark("窓と描画装置を用意する");
            // 3D のビューを GPU で描く用意をする(用意できなければ CPU で描く)。
            // GPU と CPU のどちらで描くかは描画の設定の「3D のビューの描き方」で、すぐ替わる
            // (「自動」はソフトウェアの描画装置なら CPU。gpu3d::get)。
            // 安全モードと JC_VIEW3D=cpu では用意せず、CPU の描画だけを使う。
            // JC_VIEW3D=gpu ならソフトウェアの装置でも「自動」で GPU の描き方を使う(クラウドでの点検用)。
            // シェーダーを作る間に起動の残りを進めるよう、別のスレッドで始める(初めて描く時に待つ)
            let choice = std::env::var("JC_VIEW3D").unwrap_or_default();
            if let Some(rs) = &cc.wgpu_render_state {
                // 描画の誤りで落ちず、次の回に描き直す
                surface::install_device_handlers(&cc.egui_ctx, &rs.device);
                if safe_mode || choice == "cpu" {
                    log::info!("3D のビューは CPU で描く");
                } else {
                    jc_ui::gpu3d::install_in_background(&cc.egui_ctx, rs, choice == "gpu", samples);
                }
            }
            timing.mark("3D のビューを用意する");
            match install_system_fonts(&cc.egui_ctx) {
                Some(f) => log::info!("日本語のフォント: {f}"),
                None => log::warn!("日本語のフォントが見つかりません"),
            }
            timing.mark("日本語のフォントを読む");
            log::info!("表示倍率: {}", cc.egui_ctx.pixels_per_point());
            if let Some(rs) = &cc.wgpu_render_state {
                let info = rs.adapter.get_info();
                log::info!(
                    "描画: {} ({:?}、{:?}、ドライバ {})",
                    info.name,
                    info.backend,
                    info.device_type,
                    info.driver
                );
                let note = renderer_note(&info, compiler, &rs.adapter.features());
                log::info!("描画の API: {note}");
                workspace.set_renderer_note(note);
            }
            Ok(Box::new(App {
                workspace: workspace.into_shared(),
                starting: data_dir,
                timing: Some((timing, 0)),
                bench,
                frames: 0,
                minimized: false,
                placement: None,
            }))
        }),
    );
    match result {
        Err(e) if !created.load(std::sync::atomic::Ordering::SeqCst) => {
            no_window(e, cpu, api.is_some())
        }
        Err(e) => fail("kieMonoBench が止まりました", &e.to_string()),
        Ok(()) => Ok(()),
    }
}

/// 窓のアイコン。読めなければ付けない(OS の既定の絵になる)。
fn icon() -> Option<std::sync::Arc<egui::IconData>> {
    eframe::icon_data::from_png_bytes(ICON_PNG)
        .map_err(|e| log::warn!("アイコンを読めません: {e}"))
        .ok()
        .map(std::sync::Arc::new)
}

/// 続けられない誤りを、ログ・標準エラー・OS の窓で知らせる(Windows のリリース版は端末を持たないので、
/// 窓が無いと黙って閉じたように見える)。
fn fail(title: &str, detail: &str) -> Result<(), String> {
    log::error!("{title}: {detail}");
    eprintln!("kie-monobench: {title}: {detail}");
    let logs = jc_session::default_data_dir()
        .map(|d| d.join("logs").display().to_string())
        .unwrap_or_else(|| "(データフォルダが分かりません)".into());
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(title)
        .set_description(format!("{detail}\n\nログ: {logs}"))
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    Err(format!("{title}: {detail}"))
}

/// DirectX 12 のシェーダーの作り方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Compiler {
    /// wgpu に任せる(DirectX 12 を使わない時もこれ)
    Auto,
    Dxc,
    Fxc,
}

/// DXC の本体。アプリの実行ファイルと同じフォルダに置く(`cargo xtask dxc` が置く。docs/verify.md)。
const DXC_DLL: &str = "dxcompiler.dll";

/// 描画の設定。`cpu` ならソフトウェアの描画装置(Windows は WARP、Linux は lavapipe・llvmpipe)を選ぶ。
/// `no_vsync` なら画面の書き換えの周期を待たない(速さを測る時、垂直同期を切った時)。
/// `device` は描画の設定の描画装置(高性能・省電力の GPU の選び分け)。`api` は描画の API
/// (`None` は wgpu の既定の選び方。Windows 以外では Vulkan だけを見る)。環境変数 `WGPU_BACKEND`・
/// `WGPU_DX12_COMPILER` があれば、そちらを使う(調べる時のため)。
fn wgpu_options(
    cpu: bool,
    no_vsync: bool,
    device: jc_ui::view3d_render::Device,
    api: Option<Api>,
) -> (eframe::egui_wgpu::WgpuConfiguration, Compiler) {
    use jc_ui::view3d_render::Device;
    let mut options = eframe::egui_wgpu::WgpuConfiguration {
        on_surface_status: std::sync::Arc::new(surface::surface_action),
        ..Default::default()
    };
    if no_vsync {
        options.surface.present_mode = eframe::wgpu::PresentMode::AutoNoVsync;
    }
    let mut compiler = Compiler::Auto;
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
        match device {
            Device::HighPerformance => {
                setup.power_preference = eframe::wgpu::PowerPreference::HighPerformance;
            }
            Device::LowPower => setup.power_preference = eframe::wgpu::PowerPreference::LowPower,
            // 自動は描画の部品の既定(環境変数 WGPU_POWER_PREF も効く)。ソフトウェアは下で選ぶ
            Device::Auto | Device::Software => {}
        }
        if cpu {
            // 環境変数で描画の方式を絞っていても、ソフトウェアの装置がある方式を全部見る
            setup.instance_descriptor.backends = eframe::wgpu::Backends::all();
            setup.native_adapter_selector = Some(std::sync::Arc::new(pick_cpu_adapter));
        } else if let Some(a) = api {
            compiler = choose_api(&mut setup.instance_descriptor, a);
        }
    }
    (options, compiler)
}

/// 描画の API `a` だけを探すようにし、DirectX 12 ならシェーダーの作り方を決める。
fn choose_api(desc: &mut eframe::wgpu::InstanceDescriptor, a: Api) -> Compiler {
    use eframe::wgpu::{Backends, Dx12Compiler};
    let dx12 = matches!(a, Api::Dx12Dxc | Api::Dx12Fxc);
    // 使う API だけを探す(全部を探すと、使わない Vulkan の読み込みにも時間を使う。P6)。
    // Windows 以外の DirectX 12 は、既定の選び方のまま
    let backends = match a {
        _ if dx12 && !cfg!(windows) => None,
        Api::Vulkan => Some(Backends::VULKAN),
        _ => Some(Backends::DX12),
    };
    if let Some(b) = backends
        && std::env::var_os("WGPU_BACKEND").is_none()
    {
        desc.backends = b;
    }
    let mut compiler = Compiler::Auto;
    if cfg!(windows) && dx12 && std::env::var_os("WGPU_DX12_COMPILER").is_none() {
        let dll = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join(DXC_DLL)))
            .filter(|p| p.is_file());
        desc.backend_options.dx12.shader_compiler = match (a, dll) {
            (Api::Dx12Dxc, Some(path)) => {
                compiler = Compiler::Dxc;
                Dx12Compiler::DynamicDxc {
                    dxc_path: path.display().to_string(),
                }
            }
            (Api::Dx12Dxc, None) => {
                log::warn!(
                    "{DXC_DLL} がアプリの横に無いので、シェーダーは FXC で作る(cargo xtask dxc で置ける)"
                );
                compiler = Compiler::Fxc;
                Dx12Compiler::Fxc
            }
            _ => {
                compiler = Compiler::Fxc;
                Dx12Compiler::Fxc
            }
        };
    }
    log::info!(
        "描画の API {}: 探す API {:?}、シェーダーの作り方 {compiler:?}",
        a.label(),
        desc.backends
    );
    compiler
}

/// 設定の窓に出す、今の描画の API と装置(例:「DirectX 12(DXC)、NVIDIA GeForce RTX 4060。
/// 上位の機能(DirectX 12 Ultimate の段): メッシュシェーダー 使える・レイトレーシング 使える」)。
fn renderer_note(
    info: &eframe::wgpu::AdapterInfo,
    compiler: Compiler,
    features: &eframe::wgpu::Features,
) -> String {
    use eframe::wgpu::{Backend, Features};
    let api = match (info.backend, compiler) {
        (Backend::Dx12, Compiler::Dxc) => "DirectX 12(DXC)".to_string(),
        (Backend::Dx12, Compiler::Fxc) => "DirectX 12(FXC)".to_string(),
        (Backend::Dx12, Compiler::Auto) => "DirectX 12(作り方は wgpu が選ぶ)".to_string(),
        (Backend::Vulkan, _) => "Vulkan".to_string(),
        (Backend::Gl, _) => "OpenGL".to_string(),
        (Backend::Metal, _) => "Metal".to_string(),
        (b, _) => format!("{b:?}"),
    };
    let soft = if info.device_type == eframe::wgpu::DeviceType::Cpu {
        "(ソフトウェア)"
    } else {
        ""
    };
    let can = |f: Features| {
        if features.contains(f) {
            "使える"
        } else {
            "使えない"
        }
    };
    format!(
        "{api}、{}{soft}。上位の機能(DirectX 12 Ultimate の段): メッシュシェーダー {}・レイトレーシング {}",
        info.name,
        can(Features::EXPERIMENTAL_MESH_SHADER),
        can(Features::EXPERIMENTAL_RAY_QUERY),
    )
}

/// ソフトウェアの描画装置を選ぶ。なければ、使える最初の装置。
fn pick_cpu_adapter(
    adapters: &[eframe::wgpu::Adapter],
    surface: Option<&eframe::wgpu::Surface<'_>>,
) -> Result<eframe::wgpu::Adapter, String> {
    let usable = |a: &&eframe::wgpu::Adapter| surface.is_none_or(|s| a.is_surface_supported(s));
    let names: Vec<String> = adapters.iter().map(|a| a.get_info().name).collect();
    log::info!("描画装置の候補: {names:?}");
    adapters
        .iter()
        .filter(usable)
        .find(|a| a.get_info().device_type == eframe::wgpu::DeviceType::Cpu)
        .or_else(|| adapters.iter().find(usable))
        .cloned()
        .ok_or_else(|| format!("使える描画装置がありません(候補: {names:?})"))
}

/// 窓が開かなかった(描画装置がない、装置が動かない)。設定で選んだ API なら wgpu の既定の選び方で、
/// 既定の選び方ならソフトウェア描画で 1 度だけ開き直し、それもだめなら何が起きたかを窓で知らせる
/// (黙って閉じない)。
fn no_window(error: eframe::Error, tried_cpu: bool, chosen: bool) -> Result<(), String> {
    log::error!("窓を開けません: {error}");
    eprintln!("kie-monobench: 窓を開けません: {error}");
    // 設定で選んだ API で開けなければ、先に wgpu の既定の選び方で開き直す
    // (その子も開けなければ、子がソフトウェア描画で開き直す)
    if chosen && let Ok(exe) = std::env::current_exe() {
        log::warn!("既定の選び方で開き直す({}=auto)", startup::RENDERER_ENV);
        match std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .env(startup::RENDERER_ENV, "auto")
            .env(RETRY_ENV, error.to_string())
            .status()
        {
            Ok(status) if status.success() => return Ok(()),
            Ok(_) => return Err(error.to_string()),
            Err(e) => log::error!("既定の選び方で開き直せません: {e}"),
        }
    }
    if !tried_cpu && let Ok(exe) = std::env::current_exe() {
        log::warn!("ソフトウェア描画で開き直す({}=cpu)", startup::RENDERER_ENV);
        match std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .env(startup::RENDERER_ENV, "cpu")
            .env(RETRY_ENV, error.to_string())
            .status()
        {
            Ok(status) if status.success() => return Ok(()),
            // 2 回目は自分で知らせている
            Ok(_) => return Err(error.to_string()),
            Err(e) => log::error!("ソフトウェア描画で開き直せません: {e}"),
        }
    }
    let logs = jc_session::default_data_dir()
        .map(|d| d.join("logs").display().to_string())
        .unwrap_or_else(|| "(データフォルダが分かりません)".into());
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("kieMonoBench を開けません")
        .set_description(format!(
            "画面を開けませんでした。\n\n{error}\n\n\
             グラフィックのドライバを入れ直すか、--safe-mode を付けて起動してみてください。\n\
             ログ: {logs}"
        ))
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    Err(error.to_string())
}

/// 窓の見える枠 `win`(左・上・右・下の画素)を、モニターの作業域 `work`(タスクバーを除いた所)に
/// 収めた枠。大きすぎれば作業域の大きさに縮め、はみ出していれば中へずらす(収まっていればそのまま)。
#[cfg_attr(not(windows), allow(dead_code))]
fn fit_rect(win: [i32; 4], work: [i32; 4]) -> [i32; 4] {
    let w = (win[2] - win[0]).min(work[2] - work[0]);
    let h = (win[3] - win[1]).min(work[3] - work[1]);
    let x = win[0].clamp(work[0], work[2] - w);
    let y = win[1].clamp(work[1], work[3] - h);
    [x, y, x + w, y + h]
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::Storage as _;

    struct Mem(std::collections::HashMap<String, String>);

    impl eframe::Storage for Mem {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    /// 外したモニターの位置やタスクバーの下に残った窓は、作業域の中へ戻す
    #[test]
    fn a_window_outside_the_work_area_is_moved_inside() {
        let work = [0, 0, 1920, 1040];
        // 収まっている窓はそのまま
        assert_eq!(fit_rect([100, 100, 900, 700], work), [100, 100, 900, 700]);
        // 外したモニターの位置(右の画面にあった)
        assert_eq!(
            fit_rect([2500, 200, 3300, 800], work),
            [1120, 200, 1920, 800]
        );
        // 上の帯が画面の上の外
        assert_eq!(fit_rect([100, -300, 900, 300], work), [100, 0, 900, 600]);
        // 作業域より大きい窓は縮める
        assert_eq!(fit_rect([-50, -50, 2050, 1150], work), [0, 0, 1920, 1040]);
    }

    /// 最小化する前の状態を、eframe が書くのと同じ形で書く(表示倍率 2 で最大化していた窓)
    #[test]
    fn placement_is_written_like_eframe() {
        let p = Placement {
            outer: egui::Rect::from_min_size(egui::pos2(-4.0, -4.0), egui::vec2(1928.0, 1040.0)),
            inner: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1920.0, 1032.0)),
            ppp: 2.0,
            maximized: true,
            fullscreen: false,
        };
        // 利用者の PC で eframe が app.ron に書いた形と同じ並び
        let eframe_wrote = "(inner_position_pixels:Some((x:0.0,y:0.0)),outer_position_pixels:Some((x:-8.0,y:-8.0)),fullscreen:false,maximized:true,inner_size_points:Some((x:1920.0,y:1032.0)))";
        let mut s = Mem(Default::default());
        s.set_string("ours", p.ron());
        s.set_string("eframe", eframe_wrote.to_owned());
        let ours: Option<serde_json::Value> = eframe::get_value(&s, "ours");
        let theirs: Option<serde_json::Value> = eframe::get_value(&s, "eframe");
        assert!(theirs.is_some());
        assert_eq!(ours, theirs);
    }
}
