//! 画面の枠組み。上に作業空間の切替とタブ付きツールバー、左にブラウザ、
//! 下にタイムライン、右に編集パネル、中央にスケッチか 3D のビューを置く。
//!
//! 画面の状態は [`Workspace`] が持ち、egui への依存はこの crate に閉じ込める。
//! 文書の変更は kmb-session の [`kmb_session::edit`](CLI・MCP と同じ部品)で作り、
//! 操作の中身はモジュールの拡張点([`kmb_core::Extension`])に任せる。
//! 実行ファイル(kmb-app)は、ウィンドウを開いて [`Workspace::ui`] を呼ぶだけにする。

pub mod app_icon;
mod appearance_ui;
#[doc(hidden)]
pub mod board;
mod cmdline;
mod commands_ui;
pub mod complete;
mod doc_state;
mod dock_ui;
pub mod drive;
mod editor;
mod fonts;
mod frame_ui;
mod gesture;
mod help_ui;
mod icons;
pub mod keys;
pub mod layout;
mod live_ui;
mod menus_ui;
mod mouse3d;
mod outer_ui;
mod plain_error;
pub mod ribbon;
mod settings_ui;
mod tabs_ui;
pub mod theme;
pub mod themes;
mod titlebar_ui;
mod window_state;
use window_state::Tone;
mod workspace;

// 作業空間ごとの画面(フォルダ)。直下のファイルは窓の枠・ツールバー・キー・色など、どの作業空間にも掛かる骨組み。
mod animation;
mod assembly;
mod design;
mod files;
mod machining;
mod render;
mod simulation;
use animation::animation_ui;
use assembly::{joint_drag_ui, joint_preview_ui, motion_ui, table_ui};
pub use design::picture_ui;
use design::{
    actions_ui, browser_ui, doc_ui, feature_clip_ui, feature_ui, measure_ui, panel_ui,
    param_views_ui,
    params_ui::{FieldEdit, reason_label, star_button},
    press_pull_ui, review_ui, section_ui, select_ui, timeline_ui, value_box_ui, view_ui,
};
use files::{compare_ui, files_ui, home, home_ui, versions_ui};
use machining::reuse_ui;
use render::{gallery_ui, looks_ui, render_ui};
use simulation::result_ui;

pub use commands_ui::{Command, CommandId};
pub use doc_ui::StepUp;
pub use editor::{
    APP_ADD_PARAM, APP_EDITS, APP_FINISH, APP_PARAMS, APP_RECOMPUTE, APP_REDO, APP_RUN, APP_SEARCH,
    APP_UNDO, EditorHost, EditorTool, FeatureEditor, ToolGroup,
};
pub use feature_ui::with_unit_of;
pub use files_ui::{Dialogs, NoDialogs, RecentAction, Sample, Unsaved};
pub use fonts::{install_fonts, install_system_fonts};
pub use home_ui::{
    DesignDrag, HomeHost, HomePage, HomeRequest, RecentEntry, RecoveryEntry, file_name,
};
/// 3D のビューの部品(kmb-ui-view3d)。画面の拡張とアプリが `kmb_ui::view3d` などで使えるよう、そのまま出す。
pub use kmb_ui_view3d::{gpu3d, perf, render as view3d_render, view3d};
pub use layout::{Layout, MiddleDrag, Projection, RenderSettings, Shading, Space};
pub use live_ui::LiveState;
pub use tabs_ui::KernelFactory;
pub use versions_ui::paint_fit;
pub use view3d::Pick;

/// 測る計算(画面を持たない作業と共通)。
use kmb_measure as measure;

/// 編集器が Tab を自分の欄の切り替えに使う時に、毎コマ呼ぶ。次のコマで Tab が画面の欄へ入力を
/// 移さないようにする(何も入力していない時だけ。egui は Tab で最初の欄に入力を移すため)。
pub fn keep_tab(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(keep_tab_id(), true));
}

fn keep_tab_id() -> egui::Id {
    egui::Id::new("jc_ui_keep_tab")
}

/// 文書ごとの画面の記憶(欄に打っている途中の字・通らなかった理由・引いている途中の機能など)の
/// 鍵。今のタブの通し番号を混ぜるので、タブを替えると別の記憶になり、ほかの文書の同じ番号の
/// 機能・変数へ移らない。部品の中で `ui` から作る鍵は、部品を描く所でタブごとにしてある
/// (`Workspace::pane_ui`)。窓全体の物(窓の位置・設定・クリップボード)は `egui::Id::new` のまま。
pub fn doc_id(ctx: &egui::Context, key: impl std::hash::Hash + std::fmt::Debug) -> egui::Id {
    let tab = ctx.data(|d| d.get_temp::<u64>(doc_scope_id())).unwrap_or(0);
    egui::Id::new(("jc_doc", tab)).with(key)
}

fn doc_scope_id() -> egui::Id {
    egui::Id::new("jc_ui_doc_scope")
}

/// 今のタブを [`doc_id`] の鍵に混ぜる(描き始めとタブを替えた時)。
pub(crate) fn set_doc_scope(ctx: &egui::Context, tab: u64) {
    ctx.data_mut(|d| d.insert_temp(doc_scope_id(), tab));
}

use kmb_core::{
    Document, Extensions, FeatureId, FeatureStatus, Mesh, Modules, ParamId, ParamStatus,
    RecomputeReport, Registry, ShapeHandle, Transaction, Value,
};
use kmb_session::edit;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// 3D のビューに出すスケッチの線の種類(作図の線として水色に描く)。
const SKETCH_LINE: kmb_core::LineKind =
    kmb_core::LineKind::new(kmb_interchange::ink::line::CONSTRUCTION, "スケッチ");

/// ボディごとの辺(辺を選ぶのに使う)。
type BodyEdges = Vec<(FeatureId, Vec<kmb_core::EdgeInfo>)>;

pub struct Workspace {
    /// 今のタブの文書ごとの状態(タブを替えると丸ごと入れ替える。doc_state.rs)
    cur: doc_state::DocState,
    /// 窓全体・PC 全体の状態(設定・画面の配置・知らせ・開いているタブの並び。window_state.rs)
    win: window_state::WinState,
    /// 画面の部品の並び(ドック)と、外の窓・全画面の状態(窓に 1 つ)。
    docking: dock_ui::Docking,
    /// コマンド検索の窓の状態(S で開く)。
    search: commands_ui::CommandSearch,
    /// 設定の窓の状態(開いているか・検索の字・キーの付け替えの途中など)。
    settings: settings_ui::SettingsWindow,
    /// ブラウザの部品の状態(検索・絞り込み・ドラッグで落とす所)。
    browser: browser_ui::BrowserPart,
    /// 3D のビューの右クリックと長押しのメニューの状態(画面の上の位置を持つ。タブを替えると閉じる)。
    view_menu: view_ui::ViewMenu,
    /// 開始画面(ホーム)とモデル管理の状態(home.rs)。
    home: home::HomeState,
    /// Claude が画面につなぐ(候補 21。タブを替えても続く)
    live: live_ui::LiveLink,
    /// コマンド欄(打ったコマンド・流しているスクリプト)
    cmdline: cmdline::CmdLine,
    /// 縮小図の絵の控え
    thumbs: versions_ui::Thumbs,
}

/// 「タイムライン内を検索」「ブラウザ内を検索」で探した物。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Found {
    /// タイムラインの機能(面・辺を作った機能)
    Timeline(FeatureId),
    /// ブラウザのボディ(最初に作った機能)
    Browser(FeatureId),
}

/// 3D のビューで選ぶもの(左上の切り替えか、Ctrl+右クリックで替える)。頂点・辺・面は混ぜて選べ、
/// ボディとは混ぜない(ボディを選ぶ間は、頂点・辺・面の切り替えを覚えたまま使わない)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickFilter {
    pub vertex: bool,
    pub edge: bool,
    pub face: bool,
    pub body: bool,
}

impl Default for PickFilter {
    /// 頂点・辺・面を全部選べる
    fn default() -> Self {
        PickFilter {
            vertex: true,
            edge: true,
            face: true,
            body: false,
        }
    }
}

impl PickFilter {
    /// 面だけ
    pub const FACE: PickFilter = PickFilter {
        vertex: false,
        edge: false,
        face: true,
        body: false,
    };
    /// 辺だけ
    pub const EDGE: PickFilter = PickFilter {
        vertex: false,
        edge: true,
        face: false,
        body: false,
    };
    /// 頂点だけ
    pub const VERTEX: PickFilter = PickFilter {
        vertex: true,
        edge: false,
        face: false,
        body: false,
    };
    /// ボディ
    pub const BODY: PickFilter = PickFilter {
        vertex: true,
        edge: true,
        face: true,
        body: true,
    };

    /// 頂点・辺・面のうち選べる物(ボディを選ぶ間は何も無い)。
    pub fn vertex_on(self) -> bool {
        self.vertex && !self.body
    }
    pub fn edge_on(self) -> bool {
        self.edge && !self.body
    }
    pub fn face_on(self) -> bool {
        self.face && !self.body
    }
}

/// 3D のビューの右クリックのメニューの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum MenuKind {
    #[default]
    Actions,
    /// Shift+右: 作るもの
    Create,
    /// Ctrl+右: 選ぶ種類
    Filter,
}

impl Workspace {
    /// 再計算をその場で行う作業場(テストや画面を開かない用途)。
    pub fn new(doc: Document, path: Option<PathBuf>, modules: Modules) -> Self {
        Self::open(doc, path, false, modules)
    }

    /// `background` が真なら、再計算を別のスレッドで行い、画面を止めない。
    /// `modules` は使う機能の種類の表と形状カーネル(組み立ては kmb-bundle が行う)。
    /// 作業場は具体的なカーネルを知らない。
    pub fn open(doc: Document, path: Option<PathBuf>, background: bool, modules: Modules) -> Self {
        let Modules {
            registry,
            kernel,
            extensions,
        } = modules;
        // 開いた時に寸法の名前をそろえておく(名前を付けただけでは変更にしない)
        let mut doc = doc;
        doc.settle_names(&registry);
        let saved = doc.data().clone();
        let mut ws = Workspace {
            cur: doc_state::DocState::new(
                doc,
                path,
                saved,
                Modules {
                    registry: registry.clone(),
                    kernel,
                    extensions: extensions.clone(),
                },
            ),
            win: window_state::WinState {
                crash_page: Default::default(),
                registry,
                message: None,
                message_tone: Tone::Info,
                extensions,
                background,
                repaint: None,
                dialogs: Box::new(NoDialogs),
                data_dir: None,
                safe_mode: false,
                renderer_note: None,
                mouse3d: None,
                view_rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
                view_panes: None,
                history: Vec::new(),
                history_at: Vec::new(),
                history_tone: Vec::new(),
                left_drag: view3d::LeftDrag::Select,
                focus_mode: None,
                play_speed: 10.0,
                look: theme::Look::default(),
                themes: themes::Themes::default(),
                font_applied: None,
                notices_seen: 0,
                undo_history_open: false,
                guide_open: false,
                last_item: None,
                keymap: Default::default(),
                keymap_src: None,
                layout: Layout::default(),
                layout_saved: Layout::default(),
                decal_images: Default::default(),
                tab: String::new(),
                perf: perf::Meter::default(),
                pick_filter: PickFilter::default(),
                show_params: false,
                timeline_vertical: false,
                shared: None,
                decorations_sent: None,
                canvas_target: render_ui::DEFAULT_TARGET,
                doc_tabs: vec![tabs_ui::DocTab {
                    uid: 0,
                    parked: None,
                }],
                doc_tab: 0,
                tab_seq: 0,
                kernel_factory: None,
            },
            docking: dock_ui::Docking {
                tree: None,
                spots: std::collections::BTreeMap::new(),
                shape: None,
                custom: false,
                area: egui::Vec2::ZERO,
                band_space: None,
                band_heights: std::collections::HashMap::new(),
                press: Vec::new(),
                fit: Vec::new(),
                drag: None,
                outer_start: std::collections::BTreeMap::new(),
                focus_tree: None,
            },
            search: commands_ui::CommandSearch {
                text: None,
                at: None,
                view_tools: Vec::new(),
            },
            settings: settings_ui::SettingsWindow {
                open: false,
                query: String::new(),
                key_capture: None,
                key_note: None,
                appearance_note: None,
                font_list: None,
            },
            browser: browser_ui::BrowserPart {
                query: String::new(),
                kind: browser_ui::Kind::All,
                open_folder: false,
                drop_rows: Vec::new(),
                component_drop_rows: Vec::new(),
                marked: Vec::new(),
            },
            view_menu: view_ui::ViewMenu {
                kind: MenuKind::Actions,
                candidates: None,
                context_others: (egui::Pos2::ZERO, Vec::new()),
            },
            home: home::HomeState {
                start: false,
                recoveries: Vec::new(),
                projects: Default::default(),
                recent: kmb_session::Recent::default(),
                recent_missing: Default::default(),
                samples: Vec::new(),
                page: None,
                undo: Vec::new(),
                changed: 0,
                added: None,
                drawn: false,
                panel_pass: 0,
                filters: Default::default(),
            },
            live: Default::default(),
            cmdline: Default::default(),
            thumbs: Default::default(),
        };
        ws.recompute();
        if let Some(id) = ws.last_of_space() {
            ws.select(id);
        }
        ws
    }

    /// 開いた時に選ぶ機能: 今の作業空間に出る機能のうち最後の物(設計では加工の操作を選ばない。
    /// 選ぶと設計の 3D のビューに経路が出て、タイムラインには札が無かった)。
    pub(crate) fn last_of_space(&self) -> Option<FeatureId> {
        let features = &self.cur.work.doc().data().features;
        let space = self.win.layout.space;
        let study = |t: &str| {
            self.win
                .registry
                .descriptor(t)
                .is_some_and(|d| d.category == kmb_core::category::STUDY)
        };
        features
            .iter()
            .rev()
            .find(|f| match space {
                Space::Design | Space::Render | Space::Animation => {
                    !self.is_cam_feature(&f.type_id)
                }
                Space::Machining => !study(&f.type_id),
                Space::Simulation => study(&f.type_id),
            })
            .or(features.last())
            .map(|f| f.id)
    }

    /// 画面の速さを測る時に、視点を少しずつ動かす(毎フレーム呼ぶ)。`top` なら上から見たまま
    /// 左右へ動かし、そうでなければ等角のまわりで回す。
    /// 最後に描いたフレームの、画面全体の処理・3D のビューを描く・拾うのにかかった時間。
    pub fn last_frame(&self) -> Option<perf::Frame> {
        self.win.perf.last()
    }

    /// 視点の向きを替える(ヨー・ピッチは度。軸の玉を押したのと同じ向き。画面の写真に使う)。
    pub fn set_view(&mut self, yaw_deg: f32, pitch_deg: f32) {
        self.cur.camera.set_view(yaw_deg, pitch_deg);
    }

    pub fn nudge_view(&mut self, top: bool, frame: u32) {
        let swing = ((frame % 120) as f32 - 60.0).abs() - 30.0;
        if top {
            self.cur.camera.set_view(-90.0, 90.0);
            self.cur.camera.pan = egui::vec2(swing * 3.0, 0.0);
        } else {
            self.cur.camera.set_view(-60.0 + swing, 30.0);
            self.cur.camera.pan = egui::Vec2::ZERO;
        }
    }

    pub fn document(&self) -> &Document {
        self.cur.work.doc()
    }

    /// 専用の編集器を組み込む。
    pub fn with_editor(mut self, e: impl FeatureEditor) -> Self {
        let mut e: Box<dyn FeatureEditor> = Box::new(e);
        e.set_data_dir(self.win.data_dir.as_deref());
        self.cur.editors.push(e);
        self
    }

    /// 組み込んだ編集器のうち、型が `E` のもの。
    pub fn editor<E: FeatureEditor>(&self) -> Option<&E> {
        self.cur
            .editors
            .iter()
            .find_map(|e| e.as_any().downcast_ref())
    }

    pub fn editor_mut<E: FeatureEditor>(&mut self) -> Option<&mut E> {
        self.cur
            .editors
            .iter_mut()
            .find_map(|e| e.as_any_mut().downcast_mut())
    }

    /// 編集器を一時的に取り出して、文書の状態と一緒に使う。
    fn with_editors<R>(
        &mut self,
        f: impl FnOnce(&mut [Box<dyn FeatureEditor>], &mut EditorHost<'_>) -> R,
    ) -> R {
        let mut eds = std::mem::take(&mut self.cur.editors);
        let mut host = EditorHost {
            data: self.cur.work.doc().data(),
            report: self.cur.work.report(),
            registry: &self.win.registry,
            kernel: self.cur.work.kernel(),
            data_dir: self.win.data_dir.as_deref(),
            message: &mut self.win.message,
            then: &mut self.cur.then_action,
            dialogs: &*self.win.dialogs,
            hidden: &self.cur.hidden,
            projection: self.win.layout.projection,
        };
        let r = f(&mut eds, &mut host);
        self.cur.editors = eds;
        r
    }

    /// 止まっている機能 `id` の原因: 上流をたどって最初に失敗(誤り・抑制)した機能。
    pub fn root_cause(&self, id: FeatureId) -> Option<FeatureId> {
        let mut cur = id;
        // 依存は前にしか向かないので、機能の数だけたどれば必ず終わる
        for _ in 0..=self.cur.work.doc().data().features.len() {
            match self.cur.work.report().feature(cur)? {
                FeatureStatus::Blocked { feature, .. } => cur = (*feature)?,
                _ => return (cur != id).then_some(cur),
            }
        }
        None
    }

    /// これまでに出た知らせ(新しいものが後ろ)。
    pub fn message_history(&self) -> &[String] {
        &self.win.history
    }

    /// 式を今のパラメータの値で計算する(設定欄に値と単位を出すため)。
    pub fn eval_expr(&self, src: &str) -> Result<kmb_core::Quantity, String> {
        // パラメータと寸法の名前(`d1` など)
        let values = self.cur.work.report().param_values();
        kmb_core::eval_str(src, &|n| values.get(n).copied()).map_err(|e| e.to_string())
    }

    pub fn report(&self) -> &RecomputeReport {
        self.cur.work.report()
    }

    /// 文書の長さの単位(設計の属性。候補 19)。
    pub fn length_unit(&self) -> kmb_core::LengthUnit {
        self.cur.work.doc().data().properties.units
    }

    /// 量を画面に出す言葉(長さは文書の単位で)。
    pub(crate) fn shown(&self, q: kmb_core::Quantity) -> String {
        self.length_unit().show(q)
    }

    /// 長さの欄に数だけを打った時、文書の単位を付ける(mm の文書はそのまま。計算では数だけは mm)。
    pub(crate) fn in_doc_unit(&self, expr: &str) -> String {
        let u = self.length_unit();
        let t = expr.trim();
        if !u.is_mm() && t.parse::<f64>().is_ok() {
            format!("{t} {}", u.name())
        } else {
            expr.to_string()
        }
    }

    /// パラメータの単位の名前(長さは文書の単位)。
    pub(crate) fn param_unit_label(&self, u: kmb_core::ParamUnit) -> &'static str {
        match u {
            kmb_core::ParamUnit::Length => self.length_unit().name(),
            u => u.label(),
        }
    }

    pub fn message(&self) -> Option<&str> {
        self.win.message.as_deref()
    }

    /// 最後に開いた・保存した時と中身が違うか。次に割り当てる ID(取り消しても戻らない)は比べない。
    fn differs_from_saved(&self) -> bool {
        let now = self.cur.work.doc().data();
        kmb_core::DocumentData {
            next_id: self.cur.saved.next_id,
            ..now.clone()
        } != self.cur.saved
    }

    pub fn is_modified(&self) -> bool {
        self.cur.modified
    }

    /// 最後に残ったボディ(表示しているもの)。
    /// 機械の名前(機械の役割の設定の説明の「機械: 名前」の行。なければ空)。
    pub(crate) fn machine_name(&self) -> String {
        self.win
            .extensions
            .settings()
            .iter()
            .find(|g| g.role == Some(kmb_interchange::role::settings::MACHINE))
            .and_then(|g| {
                g.notes
                    .iter()
                    .find_map(|n| n.strip_prefix("機械: ").map(String::from))
            })
            .unwrap_or_default()
    }

    pub(crate) fn count_of(&self, type_id: &str) -> usize {
        self.cur
            .work
            .doc()
            .data()
            .features
            .iter()
            .filter(|f| f.type_id == type_id)
            .count()
    }

    pub fn layout(&self) -> &Layout {
        &self.win.layout
    }

    pub fn layout_mut(&mut self) -> &mut Layout {
        &mut self.win.layout
    }

    /// 画面を描く。毎フレーム呼ぶ。
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.win.perf.begin(std::time::Instant::now());
        self.frame(ui);
        self.win.perf.end(std::time::Instant::now());
    }

    fn frame(&mut self, ui: &mut egui::Ui) {
        // 寸法の札・式の値・測る道具は、今のタブの文書の単位で出す(候補 19)
        kmb_core::set_display_unit(self.length_unit());
        if self.win.repaint.is_none() {
            self.win.repaint = Some(ui.ctx().clone());
            if let Some(m) = &self.win.mouse3d {
                let ctx = ui.ctx().clone();
                m.set_wake(move || ctx.request_repaint());
            }
            // 右クリックとドラッグを分ける動き(4 px 以下ならクリック)
            ui.ctx()
                .options_mut(|o| o.input_options.max_click_dist = 4.0);
        }
        set_doc_scope(ui.ctx(), self.tab_uid());
        self.feed_mouse3d(ui.ctx());
        // 編集器が Tab を使っている(描いている途中の寸法の欄): 画面の欄へ入力を移さない
        let ctx = ui.ctx();
        if ctx
            .data_mut(|d| d.remove_temp::<bool>(keep_tab_id()))
            .unwrap_or(false)
            && ctx.memory(|m| m.focused().is_none())
        {
            ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
        }
        self.poll_job();
        self.poll_task();
        self.follow_editing_for_hidden();
        // 画面の部品を名前で操作する差し込み口(コマンド欄・AI の接続・バッチの e2e が使う)。
        // 部品の一覧(AccessKit の木)は、使う時だけ作る
        let c = ui.ctx().clone();
        if c.plugin_opt::<drive::UiDriver>().is_none() {
            c.add_plugin(drive::UiDriver::default());
        }
        if self.script_running()
            || self.win.layout.ai_live
            || self.pane_open(dock_ui::Pane::Commands)
        {
            c.enable_accesskit();
        }
        self.cmdline_tick(&c);
        self.live_tick();
        self.poll_recent();
        self.refresh_links(ui.ctx());
        complete::set_names(ui.ctx(), self.expr_candidates());
        // 描画の設定(U26): すぐ効く物を 3D のビューへ渡し、網の細かさが替わったら作り直す
        kmb_ui_view3d::render::set_current(&self.win.layout.render);
        crate::gpu3d::set_mode(ui.ctx(), self.win.layout.render.view3d);
        self.cur.scene.set_tolerance(
            self.cur.work.kernel().as_ref(),
            self.win.layout.render.mesh.tolerance(),
        );
        self.cur.scene.section = self
            .cur
            .section
            .as_ref()
            .filter(|s| s.open)
            .map(|s| s.section());
        self.sync_effects();
        self.sync_animation(ui.ctx());
        self.sync_joint_preview(ui.ctx());
        self.sync_drag_pose();
        self.sync_motion_study(ui.ctx());
        if let Some((id, args)) = self
            .cur
            .then_action
            .take_if(|(id, _)| id.starts_with("app:"))
        {
            // 編集器が画面に頼んだ操作(APP_FINISH のほかは直すのを終えない)
            match id.as_str() {
                APP_UNDO => self.undo(),
                APP_REDO => self.redo(),
                APP_PARAMS => self.win.show_params = true,
                APP_RECOMPUTE => self.recompute(),
                APP_SEARCH => self.open_command_palette_at(ui.ctx().pointer_latest_pos()),
                APP_FINISH => self.finish_sketch(),
                APP_RUN => {
                    // 操作の名前か、役割の印(編集器は機能の名前を書かず、役割で頼む)
                    let id = match args["id"].as_str() {
                        Some(a) => Some(a.to_string()),
                        None => args["role"]
                            .as_str()
                            .and_then(|n| {
                                kmb_interchange::role::action::ALL.iter().find(|r| r.0 == n)
                            })
                            .and_then(|r| self.action_for_role(r)),
                    };
                    // `finish` なら直すのを終えてから(スケッチは選んだまま)、`args` の値で始める
                    // (スケッチの編集器の「文字」。文字はスケッチの後の別の機能)
                    match id {
                        Some(a) if args["finish"].as_bool() == Some(true) => {
                            self.win.last_item = Some(format!("a:{a}"));
                            self.finish_edit();
                            self.wait();
                            self.start_action_with(&a, args["args"].clone());
                        }
                        Some(a) => {
                            self.run_action(&a);
                        }
                        None => {}
                    }
                }
                APP_EDITS => {
                    let label = args["label"].as_str().unwrap_or("機能を変更").to_string();
                    let edits: Vec<kmb_core::Edit> = args["edits"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|e| {
                            Some(kmb_core::Edit::SetData {
                                id: FeatureId(e["id"].as_u64()?),
                                data: e["data"].clone(),
                            })
                        })
                        .collect();
                    if !edits.is_empty() {
                        let r = self.cur.work.apply_edits(&label, edits).map(|_| ());
                        if self.after_change(r)
                            && let Some(m) = args["message"].as_str()
                        {
                            self.win.message = Some(m.to_string());
                        }
                    }
                }
                APP_ADD_PARAM => {
                    let (name, expr) = (args["name"].as_str(), args["expr"].as_str());
                    if let (Some(n), Some(e)) = (name, expr) {
                        self.add_param(n, e);
                    }
                }
                _ => {}
            }
        }
        if let Some((id, args)) = self.cur.then_action.take() {
            self.win.last_item = Some(format!("a:{id}"));
            // 編集器が頼んだ操作: 直すのを終え(スケッチは選んだまま)、決まった値から始める
            self.finish_edit();
            self.wait();
            self.start_action_with(&id, args);
        }
        if self.is_busy() || self.is_running() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.sync_keymap();
        self.shortcuts(ui.ctx());
        self.view_keys(ui.ctx());
        self.log_message();
        self.autosave_tick(std::time::SystemTime::now());
        let pref = self.win.layout.brightness.preference();
        if ui.ctx().options(|o| o.theme_preference) != pref {
            ui.ctx().set_theme(pref);
        }
        // スケッチを直している間は差し色をスケッチの水色にする(設計の画面と見分ける。レビュー K15)
        let sketching = self.view_editor_type().is_some();
        self.apply_appearance(ui.ctx(), sketching);
        let look = theme::look(ui.ctx());
        self.win.look = look;
        let bar = |fill: egui::Color32, x: i8| {
            egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(x, 0))
        };
        // 帯の高さは、設定の字とアイコンの大きさに合わせて広げる(1 倍なら試作と同じ)。
        // アプリが描く窓の帯の時は、右端の閉じるを窓の角に付ける
        self.sync_decorations(ui.ctx());
        let right = if self.custom_title_bar() { 0 } else { 10 };
        egui::Panel::top("menubar")
            .exact_size(34.0 * theme::band())
            .frame(
                egui::Frame::new()
                    .fill(look.pal.chrome2)
                    .inner_margin(egui::Margin {
                        left: 10,
                        right,
                        top: 0,
                        bottom: 0,
                    }),
            )
            .show(ui, |ui| self.menubar(ui));
        let ctx = ui.ctx().clone();
        self.command_palette(&ctx);
        self.crash_window(&ctx);
        self.guide_window(&ctx);
        self.text_viewer(&ctx);
        self.table_window(&ctx);
        self.undo_history_window(&ctx);
        self.settings_window(&ctx);
        self.group_dialog(&ctx);
        self.properties_window(&ctx);
        self.versions_window(&ctx);
        self.render_viewer_window(&ctx);
        self.motion_study_window(&ctx);
        self.looks_window(&ctx);
        self.compare_window(&ctx);
        self.opening_window(&ctx);
        self.step_up_dialog(&ctx);
        self.remove_ask_ui(&ctx);
        self.rename_feature_ui(&ctx);
        self.name_ask_ui(&ctx);
        if self.home.start {
            egui::Panel::bottom("start_status")
                .exact_size(theme::fs(24.0))
                .frame(bar(look.pal.chrome2, 12))
                .show(ui, |ui| self.start_status(ui));
            let page = egui::Frame::new().fill(look.pal.chrome);
            egui::CentralPanel::default()
                .frame(page)
                .show(ui, |ui| self.home_ui(ui));
            self.window_edges(ui);
            return;
        }
        self.home.drawn = false;
        egui::Panel::top("ribbon")
            .exact_size(theme::fs(30.0) + self.band_height())
            .frame(bar(look.pal.chrome, 0))
            .show(ui, |ui| self.ribbon_ui(ui));
        egui::Panel::bottom("status")
            .exact_size(theme::fs(24.0))
            .frame(bar(look.pal.chrome2, 12))
            .show(ui, |ui| self.status_bar(ui));
        // 3D のビュー・ブラウザ・編集パネル・タイムラインは、自由に並べ替えられる(N4)
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(look.pal.chrome2))
            .show(ui, |ui| {
                // 画面の部品の記憶(開いた木・巻き取りの位置など)は機能の番号を鍵にするので、
                // 文書のタブごとに分ける
                ui.push_id(("doc_tab", self.tab_uid()), |ui| self.dock_ui(ui));
            });
        self.window_edges(ui);
        self.save_layout_if_changed(&ctx);
    }

    /// 知らせが変わったら履歴に残す。
    fn log_message(&mut self) {
        if let Some(m) = &self.win.message
            && self.win.history.last() != Some(m)
        {
            self.win.history.push(m.clone());
            self.win.history_at.push(crate::help_ui::now());
            self.win.history_tone.push(self.win.message_tone);
            if self.win.history.len() > 100 {
                self.win.history.remove(0);
                self.win.history_at.remove(0);
                self.win.history_tone.remove(0);
            }
        }
        // 強さは 1 つの知らせのもの。次の知らせは出す所が決め直す
        self.win.message_tone = Tone::Info;
    }

    /// Ctrl のキー(文字の欄を打っている時も効く。キーの一覧は keys::KEYS)。
    fn shortcuts(&mut self, ctx: &egui::Context) {
        while let Some(row) = self.pressed_key(ctx, keys::Scope::Always) {
            self.run_key(ctx, row);
        }
    }
}

/// 操作のボタン。使えなければ灰色にして理由を出す。
pub(crate) fn action_button(ui: &mut egui::Ui, a: &kmb_core::Action) -> egui::Response {
    let r = ui.add_enabled(a.disabled.is_none(), egui::Button::new(a.label.as_str()));
    match &a.disabled {
        Some(why) => r.on_disabled_hover_text(why),
        None if !a.hint.is_empty() => r.on_hover_text(&a.hint),
        None => r,
    }
}

/// 右クリックのメニューの操作の 1 行([`theme::menu_row`]。アイコンは操作が名乗る物、`keys` は
/// その操作に割り当てたキー)。
pub(crate) fn action_row(ui: &mut egui::Ui, a: &kmb_core::Action, keys: &str) -> egui::Response {
    let r = theme::menu_row(ui, &a.icon, &a.label, keys, a.disabled.is_none());
    match &a.disabled {
        Some(why) => r.on_disabled_hover_text(why),
        None if !a.hint.is_empty() => r.on_hover_text(&a.hint),
        None => r,
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod toolbar_tests;

#[cfg(test)]
mod screen_fixes_tests;

#[cfg(test)]
mod appearance_tests;
#[cfg(test)]
mod cmdline_tests;
#[cfg(test)]
mod doc_tests;
#[cfg(test)]
mod dock_tests;
#[cfg(test)]
mod keys_tests;
#[cfg(test)]
mod menu_keys_tests;
#[cfg(test)]
mod one_path_tests;
#[cfg(test)]
mod scale_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod tabs_tests;
#[cfg(test)]
mod units_tests;
