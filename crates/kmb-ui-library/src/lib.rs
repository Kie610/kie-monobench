//! モデル管理の画面(作った物と決めたことは docs/機能/ の「ホームのタブ」「プロジェクトの仕分け」
//! 「データパネル」「整理と保存した絞り込み」)。
//!
//! ホームのタブ(左端の閉じられないタブ)の中身を描く。左に始め方と場所(最近・ピン留め・見本・
//! プロジェクトのフォルダの木)、中央に場所の中のフォルダとデザインのカード、右に選んだ物の概要。
//! 初めは「最近使ったデザイン」。
//!
//! 画面の本体(kmb-ui)とは拡張点 [`kmb_ui::HomePage`] でつなぐ。ここは描くことと、選んでいる場所・
//! 選んでいる物・絞り込みのような部品ごとの状態だけを持つ。文書を開く・最近の一覧を変える・
//! デザインを移すなどは [`kmb_ui::HomeRequest`] で頼み、本体が行う。デザインの概要・フォルダの中身を
//! 読むのは kmb-session([`kmb_session::library`]・[`kmb_session::projects`])で、CLI・MCP も同じ関数を使う。
//!
//! 仕分け(Kie 2026-10-23「a」): 左のフォルダを押すのはいつもそのフォルダを開くだけ。移すのは、
//! カードを左のフォルダかフォルダのカードへ引いて離すか、右クリックと選んだ数の帯の「移す」から。
//!
//! 整理: 左の「整理」に、同じ・似ているデザイン(組を並べ、右に組の違い)、分類のないデザイン、
//! 見つからないファイルの頁。どれも候補を並べるだけで、移す・消すのは利用者が押した時だけ。
//! 判定は kmb-session の [`kmb_session::tidy`]。絞り込みは名前を付けて保存でき、左の「保存した
//! 絞り込み」から全部のプロジェクトと最近のデザインに当てる。中央は格子と一覧(表)を切り替える。

use kmb_session::filters::Filter;
use kmb_session::projects::ModelInfo;
use kmb_ui::theme;
use kmb_ui::{HomeHost, HomePage, file_name, paint_fit};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

mod left;
mod middle;
mod panel;
mod right;
mod summaries;
mod tidy_ui;
use summaries::{Summaries, Trees};

/// はじめての手順(見本の頁と、最近使ったデザインが無い時の中央に出す)。
const FIRST_STEPS: &[&str] = &[
    "見本の中から、加工まで入ったものを開く",
    "左のブラウザの「パラメータ」で寸法(例: width)を変えると、形と加工の経路が作り直される",
    "左上の作業空間を「加工」にし、ブラウザでセットアップを選び、「確かめる」の「シミュレーション」で食い込みと削り残しを確かめる",
    "「G-code を見る」で中身を確かめ、「G-code を名前を付けて書き出す…」で保存する",
    "機械の値やポスト、マウスの割り当ては上の「設定」、変更の取り消しは「取り消しの履歴」で戻せる",
    "迷ったら S(カーソルの所)か Ctrl+K のコマンド検索(名前や読み、例えば「おしだし」で探せる)",
];

/// 左に並べる場所。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Place {
    #[default]
    Recent,
    Pinned,
    Samples,
    /// プロジェクト `project` の中のフォルダ `folder`(プロジェクトそのものも)
    Folder {
        project: PathBuf,
        folder: PathBuf,
    },
    /// 整理の頁
    Tidy(Tidy),
    /// 保存した絞り込み(名前)
    Saved(String),
}

/// 整理の頁。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tidy {
    Similar,
    Unclassified,
    Missing,
}

impl Tidy {
    fn label(self) -> &'static str {
        match self {
            Tidy::Similar => "同じ・似ているデザイン",
            Tidy::Unclassified => "分類のないデザイン",
            Tidy::Missing => "見つからないファイル",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Tidy::Similar => "copy",
            Tidy::Unclassified => "tag",
            Tidy::Missing => "warn",
        }
    }
}

/// 一覧(表)の並べ方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Sort {
    #[default]
    None,
    Name,
    Place,
    Version,
    Modified,
    Size,
}

impl Place {
    fn label(&self) -> String {
        match self {
            Place::Recent => "最近使ったデザイン".into(),
            Place::Pinned => "ピン留めしたデザイン".into(),
            Place::Samples => "見本とはじめての手順".into(),
            Place::Folder { folder, .. } => file_name(folder),
            Place::Tidy(t) => t.label().into(),
            Place::Saved(name) => name.clone(),
        }
    }
}

/// 中央に並べる物 1 つ(フォルダかデザイン)。
#[derive(Debug, Clone, PartialEq)]
struct Item {
    path: PathBuf,
    /// フォルダなら中のデザインの数(下のフォルダも)
    folder: Option<usize>,
    pinned: bool,
    /// ファイルが見つからない(最近の一覧に残っている物)
    missing: bool,
}

/// 引いているデザイン(左のフォルダかフォルダのカードへ離すと移す。データパネルから 3D のビューへ
/// 離すとリンクで挿入)。
use kmb_ui::DesignDrag as Drag;

/// 分類を直している途中の値(右の概要の欄)。
#[derive(Debug, Clone, Default, PartialEq)]
struct InfoDraft {
    title: String,
    project: String,
    genre: String,
    tags: String,
    material: String,
}

impl InfoDraft {
    fn of(i: &ModelInfo) -> Self {
        Self {
            title: i.title.clone(),
            project: i.project.clone(),
            genre: i.genre.clone(),
            tags: i.tags.join(", "),
            material: i.material.clone(),
        }
    }

    /// `base` の分類をこの値で直した物(ほかの欄は `base` のまま)。
    fn apply(&self, base: &ModelInfo) -> ModelInfo {
        let mut p = kmb_core::Properties::default();
        p.set_tags_text(&self.tags);
        ModelInfo {
            title: self.title.trim().into(),
            project: self.project.trim().into(),
            genre: self.genre.trim().into(),
            tags: p.tags,
            material: self.material.trim().into(),
            ..base.clone()
        }
    }
}

/// モデル管理の画面(ホームの中身)。
#[derive(Default)]
pub struct Library {
    place: Place,
    /// 選んでいる物(デザインは複数、フォルダは 1 つ)
    selected: Vec<PathBuf>,
    /// Shift で範囲を選ぶ時の起点
    anchor: Option<PathBuf>,
    /// 探す欄の文字
    query: String,
    period: kmb_session::library::Period,
    /// 絞り込み: ジャンル・作品・タグ(全部を持つ物)
    genre: Option<String>,
    work: Option<String>,
    tags: Vec<String>,
    summaries: Summaries,
    trees: Trees,
    /// 開いている左の木の行
    expanded: HashSet<PathBuf>,
    /// 最後に見た仕分けの回数(変われば読み直す)
    changed: u64,
    /// 「元に戻す」の帯を閉じた時の仕分けの回数
    undo_closed: Option<u64>,
    /// 新しいフォルダの名前の欄
    new_folder: String,
    /// 名前を変えている途中(デザインと新しい名前)
    renaming: Option<(PathBuf, String)>,
    /// ごみ箱へ移すか確かめている物
    confirm_trash: Option<Vec<PathBuf>>,
    /// 名前の欄に一度だけ入力の場所を移す
    rename_focus: bool,
    /// 分類を直している途中(デザインと値)と、直し始めた時の分類
    draft: Option<(PathBuf, InfoDraft)>,
    draft_base: Option<ModelInfo>,
    /// データパネル(設計の画面の左の細い形)の状態
    panel: panel::Panel,
    /// 中央を一覧(表)で出すか(偽なら格子)
    list_view: bool,
    /// 一覧の並べ方と、逆の順か
    sort: (Sort, bool),
    /// 同じ・似ているデザインの頁で選んでいる組(番号)
    group: Option<usize>,
    /// 同じ・似ているデザインの組の控え(読み直しの印と組)
    groups: Option<(GroupsKey, Vec<kmb_session::tidy::Group>)>,
    /// 絞り込みを保存する名前の欄(出している間だけ)と、一度だけ入力の場所を移す印
    filter_name: Option<String>,
    filter_focus: bool,
    /// ごみ箱へ移すか確かめているフォルダ
    confirm_trash_folder: Option<PathBuf>,
}

/// 組の控えを作り直す印(仕分けの回数・読み終えたプロジェクトの数・デザインの数)。
type GroupsKey = (u64, usize, usize);

impl Library {
    pub fn new() -> Self {
        Self::default()
    }

    /// 場所を替える(選んでいる物と途中の操作は捨てる)。
    fn go(&mut self, place: Place) {
        if let Place::Folder { project, folder } = &place {
            // 開いた所までの木の行を開く
            let mut p = folder.as_path();
            while p.starts_with(project) {
                self.expanded.insert(p.to_path_buf());
                match p.parent() {
                    Some(q) => p = q,
                    None => break,
                }
            }
        }
        self.place = place;
        self.select_none();
    }

    fn select_none(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.renaming = None;
        self.confirm_trash = None;
        self.confirm_trash_folder = None;
        self.group = None;
    }

    /// 絞り込みか探す語があるか(フォルダの中では、あればプロジェクト全体から出す)。
    fn filtering(&self) -> bool {
        !self.query.trim().is_empty()
            || self.genre.is_some()
            || self.work.is_some()
            || !self.tags.is_empty()
    }

    /// 今の絞り込み(探す語・ジャンル・作品・タグ・変更日)の写し。
    fn filter(&self) -> Filter {
        Filter {
            name: String::new(),
            query: self.query.trim().to_string(),
            genre: self.genre.clone(),
            project: self.work.clone(),
            tags: self.tags.clone(),
            period: self.period,
        }
    }

    /// `path` を含むプロジェクト。
    fn project_of(host: &HomeHost<'_>, path: &Path) -> Option<PathBuf> {
        host.projects
            .iter()
            .filter(|p| path.starts_with(p))
            .max_by_key(|p| p.components().count())
            .cloned()
    }
}

impl Library {
    /// ホームとデータパネルの描き始めに、変わった物を読み直す。
    fn sync(&mut self, host: &HomeHost<'_>) {
        if host.shown {
            // 出し直した時は、隠している間に変わった物を読み直す
            self.summaries.refresh();
            self.trees.refresh();
        }
        if host.changed != self.changed {
            // 仕分けでファイルが変わった。選んでいた物のパスは古いかもしれない
            self.changed = host.changed;
            self.summaries.refresh();
            self.trees.refresh();
            self.selected.retain(|p| p.exists());
            self.renaming = None;
            self.confirm_trash = None;
            self.draft = None;
            self.confirm_trash_folder = None;
            self.panel.changed();
        }
    }
}

impl HomePage for Library {
    fn loading(&self, pass: u64) -> bool {
        self.summaries.loading(pass) || self.trees.loading(pass)
    }

    fn panel(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        self.sync(host);
        self.panel_ui(ui, host);
        middle::paint_drag(ui.ctx());
    }

    fn ui(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        self.sync(host);
        if let Some(p) = host.added.take() {
            self.go(Place::Folder {
                project: p.clone(),
                folder: p,
            });
        }
        if let Place::Folder { project, .. } = &self.place
            && !host.projects.contains(project)
        {
            self.go(Place::Recent);
        }
        let l = theme::look(ui.ctx());
        let full = ui.max_rect();
        let left_w = 232.0 * theme::band().clamp(1.0, 1.6);
        let right_w = (300.0 * theme::band().clamp(1.0, 1.4)).min(full.width() * 0.3);
        let left = egui::Rect::from_min_size(full.min, egui::vec2(left_w, full.height()));
        let right =
            egui::Rect::from_min_max(egui::pos2(full.right() - right_w, full.top()), full.max);
        let mid = egui::Rect::from_min_max(
            egui::pos2(left.right(), full.top()),
            egui::pos2(right.left(), full.bottom()),
        );
        let p = ui.painter();
        p.rect_filled(left, 0.0, l.pal.chrome2);
        p.rect_filled(right, 0.0, l.pal.chrome2);
        let line = egui::Stroke::new(1.0, l.pal.edge);
        p.vline(left.right() - 0.5, full.y_range(), line);
        p.vline(right.left() + 0.5, full.y_range(), line);
        let col = |ui: &mut egui::Ui, r: egui::Rect, x: f32, salt: &str| {
            let mut c = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(salt)
                    .max_rect(r.shrink2(egui::vec2(x, 0.0)))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            c.set_clip_rect(r);
            c
        };
        let mut c = col(ui, left, 14.0, "home_left");
        egui::ScrollArea::vertical()
            .id_salt("home_left_scroll")
            .auto_shrink(false)
            .show(&mut c, |c| self.left(c, host));
        let mut c = col(ui, mid, 22.0, "home_mid");
        self.middle(&mut c, host);
        let mut c = col(ui, right, 16.0, "home_right");
        egui::ScrollArea::vertical()
            .id_salt("home_right_scroll")
            .auto_shrink(false)
            .show(&mut c, |c| self.right(c, host));
        middle::paint_drag(ui.ctx());
    }
}

/// キーの添え書きつきのボタン(読み上げと画面の台本の名前は `text` だけ)。
fn key_button(
    ui: &mut egui::Ui,
    primary: bool,
    text: &str,
    key: &str,
    enabled: bool,
) -> egui::Response {
    let r = if primary {
        theme::primary(ui, text, key, enabled)
    } else {
        theme::secondary(ui, text, key, enabled)
    };
    ui.ctx().accesskit_node_builder(r.id, |b| b.set_label(text));
    r
}

/// 小さな見出し(区画の名前)。
fn heading(ui: &mut egui::Ui, t: &str) {
    let l = theme::look(ui.ctx());
    ui.add_space(18.0);
    ui.label(
        egui::RichText::new(t)
            .size(theme::fs(11.0))
            .color(l.pal.ink2),
    );
    ui.add_space(4.0);
}

/// 左の大きなボタン(新規デザイン・開く…・プロジェクトを追加)。
fn big_button(ui: &mut egui::Ui, icon: &str, text: &str, primary: bool) -> egui::Response {
    let l = theme::look(ui.ctx());
    let w = ui.available_width();
    let (rect, r) =
        ui.allocate_exact_size(egui::vec2(w, 34.0 * theme::band()), egui::Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    let (fill, edge, fg) = if primary {
        (l.ws(), l.ws(), l.pal.chrome)
    } else if r.hovered() {
        (l.pal.chrome, theme::mix(l.pal.edge, l.ws(), 0.5), l.pal.ink)
    } else {
        (l.pal.panel, l.pal.edge, l.pal.ink)
    };
    let p = ui.painter();
    p.rect(
        rect,
        theme::corner(4.0),
        fill,
        egui::Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    let s = theme::icon_px(15.0);
    theme::paint_icon(
        p,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 12.0 + s / 2.0, rect.center().y),
            egui::Vec2::splat(s),
        ),
        icon,
        fg,
    );
    p.text(
        egui::pos2(rect.left() + 12.0 + s + 9.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        theme::prop(12.5),
        fg,
    );
    r
}

/// 左の場所の行(名前と右に件数。選んでいる行は塗る)。
fn place_row(
    ui: &mut egui::Ui,
    icon: &str,
    text: &str,
    count: Option<usize>,
    on: bool,
) -> egui::Response {
    let l = theme::look(ui.ctx());
    let w = ui.available_width();
    let (rect, r) =
        ui.allocate_exact_size(egui::vec2(w, 26.0 * theme::band()), egui::Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, text));
    let p = ui.painter();
    if on {
        p.rect_filled(rect, theme::corner(4.0), l.ws_soft());
    } else if r.hovered() {
        p.rect_filled(rect, theme::corner(4.0), l.pal.edge.gamma_multiply(0.35));
    }
    let fg = if on { l.pal.ink } else { l.pal.ink2 };
    let s = theme::icon_px(14.0);
    theme::paint_icon(
        p,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 8.0 + s / 2.0, rect.center().y),
            egui::Vec2::splat(s),
        ),
        icon,
        fg,
    );
    let right = match count {
        Some(n) => {
            let g = p.layout_no_wrap(n.to_string(), theme::prop(11.0), l.pal.ink3);
            let x = rect.right() - 8.0 - g.size().x;
            p.galley(
                egui::pos2(x, rect.center().y - g.size().y / 2.0),
                g,
                l.pal.ink3,
            );
            x - 6.0
        }
        None => rect.right() - 6.0,
    };
    let x = rect.left() + 8.0 + s + 8.0;
    let g = p.layout(
        text.to_string(),
        theme::prop(12.5),
        fg,
        (right - x).max(1.0),
    );
    let row = g.rows.first().map_or(0.0, |r| r.rect().height());
    p.with_clip_rect(egui::Rect::from_x_y_ranges(x..=right, rect.y_range()))
        .galley(egui::pos2(x, rect.center().y - row / 2.0), g, fg);
    r
}

/// 名前と値の行(値が空なら「—」)。
fn rows(ui: &mut egui::Ui, items: &[(&str, &str)]) {
    let l = theme::look(ui.ctx());
    egui::Grid::new(ui.next_auto_id())
        .num_columns(2)
        .spacing(egui::vec2(10.0, 3.0))
        .show(ui, |ui| {
            for (k, v) in items {
                ui.label(
                    egui::RichText::new(*k)
                        .size(theme::fs(11.5))
                        .color(l.pal.ink2),
                );
                ui.label(
                    egui::RichText::new(if v.is_empty() { "—" } else { v })
                        .size(theme::fs(11.5))
                        .color(l.pal.ink),
                );
                ui.end_row();
            }
        });
}

/// はじめての手順。
fn steps(ui: &mut egui::Ui) {
    let l = theme::look(ui.ctx());
    heading(ui, "はじめての手順");
    for (k, step) in FIRST_STEPS.iter().enumerate() {
        ui.label(
            egui::RichText::new(format!("{}. {step}", k + 1))
                .size(theme::fs(11.5))
                .color(l.pal.ink),
        );
    }
}

/// 見本のボタン(箱の絵と名前)。
fn big_sample(ui: &mut egui::Ui, name: &str) -> egui::Response {
    let l = theme::look(ui.ctx());
    let w = 188.0 * theme::band().clamp(1.0, 1.6);
    let (rect, r) =
        ui.allocate_exact_size(egui::vec2(w, 120.0 + theme::fs(26.0)), egui::Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    let p = ui.painter();
    let edge = if r.hovered() { l.ws() } else { l.pal.edge };
    p.rect(
        rect,
        theme::corner(6.0),
        l.pal.panel,
        egui::Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    let thumb =
        egui::Rect::from_min_size(rect.min + egui::vec2(1.0, 1.0), egui::vec2(w - 2.0, 118.0));
    p.rect_filled(thumb, theme::corner(5.0), l.pal.canvas);
    plate(p, thumb, &l);
    p.text(
        egui::pos2(rect.left() + 10.0, thumb.bottom() + theme::fs(13.0)),
        egui::Align2::LEFT_CENTER,
        name,
        theme::prop(12.5),
        l.pal.ink,
    );
    r.on_hover_text("見本を新しいタブで開く")
}

/// 縮小図の無いデザインの絵(等角の板)。
fn plate(pa: &egui::Painter, r: egui::Rect, l: &theme::Look) {
    let c = r.center();
    let (a, b, h) = (36.0, 18.0, 6.0);
    let top = [
        c + egui::vec2(-a, 0.0),
        c + egui::vec2(0.0, -b),
        c + egui::vec2(a, 0.0),
        c + egui::vec2(0.0, b),
    ];
    let line = egui::Stroke::new(1.0, l.pal.shade2);
    pa.add(egui::Shape::convex_polygon(
        top.to_vec(),
        l.pal.shade3,
        line,
    ));
    pa.add(egui::Shape::convex_polygon(
        vec![
            top[0],
            top[3],
            top[3] + egui::vec2(0.0, h),
            top[0] + egui::vec2(0.0, h),
        ],
        l.pal.shade1,
        line,
    ));
    pa.add(egui::Shape::convex_polygon(
        vec![
            top[3],
            top[2],
            top[2] + egui::vec2(0.0, h),
            top[3] + egui::vec2(0.0, h),
        ],
        l.pal.shade2,
        line,
    ));
}

#[cfg(test)]
mod tests;
