//! 下のタイムライン(左から古い順)。機能のグループは色の枠と名前の見出しで囲み、畳んでも
//! 名前・数・中身の印を出す。ロールバックの棒は右クリックか矢印ボタンで動かす。
//! 機能はドラッグで並べ替えられる(グループの範囲の中へ動かせば、そのグループに入る)。
//!
//! 縦長の所に置いた時は、上から古い順の縦の一覧にする(行にアイコンと名前、今の位置の印は行の間の横の棒で上下へ引く)。

use crate::{Found, Workspace, icons, theme};
use egui::{Rect, Sense, Stroke, Vec2};
use kmb_core::{FeatureId, FeatureStatus, Group};
use kmb_session::edit::{self, GroupError, GroupMode};

/// グループの色(番号で回す。試作の `.tlg`。色はテーマの `group_1`〜`group_3`)。
fn group_color(id: u64, l: &theme::Look) -> egui::Color32 {
    let c = l.pal.inks.group;
    c[(id as usize) % c.len()]
}

/// 機能の状態の印と説明。
pub(crate) fn status_mark(status: Option<&FeatureStatus>) -> (&'static str, String) {
    match status {
        Some(FeatureStatus::Ok { outputs, .. }) => {
            let w = kmb_core::warnings_of(outputs);
            if w.is_empty() {
                ("✔", String::new())
            } else {
                ("⚠", w.join("\n"))
            }
        }
        Some(FeatureStatus::Error { message }) => ("✖", message.clone()),
        Some(FeatureStatus::Blocked { by, .. }) => ("⏸", format!("{by} が使えません")),
        Some(FeatureStatus::Suppressed) => ("–", "抑制中".into()),
        Some(FeatureStatus::RolledBack) => ("…", "ロールバック位置より後ろ".into()),
        None => ("?", String::new()),
    }
}

/// 機能の計算の具合。タイムライン・ブラウザの印の形と色、確かめる窓の中身を決める。
/// 失敗(この機能が計算できない)と、前の機能のせいで止まったものと、注意(計算はできた)を分ける。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Health {
    Ok,
    /// 計算できたが注意がある
    Warning(String),
    /// この機能そのものが計算できない
    Error(String),
    /// 前の機能(やパラメータ)が使えないので計算しなかった
    Blocked {
        by: String,
        feature: Option<FeatureId>,
    },
    /// 抑制中・ロールバックの後・まだ計算していない
    Idle,
}

impl Health {
    pub(crate) fn of(status: Option<&FeatureStatus>) -> Health {
        match status {
            Some(FeatureStatus::Ok { outputs, .. }) => {
                let w = kmb_core::warnings_of(outputs);
                if w.is_empty() {
                    Health::Ok
                } else {
                    Health::Warning(w.join("\n"))
                }
            }
            Some(FeatureStatus::Error { message }) => Health::Error(message.clone()),
            Some(FeatureStatus::Blocked { by, feature }) => Health::Blocked {
                by: by.clone(),
                feature: *feature,
            },
            _ => Health::Idle,
        }
    }

    /// 印を目立たせる問題か(注意・エラー・止まっている)。
    pub(crate) fn is_problem(&self) -> bool {
        matches!(
            self,
            Health::Warning(_) | Health::Error(_) | Health::Blocked { .. }
        )
    }

    /// 赤で出すか(エラーか、止まっている)。
    pub(crate) fn is_red(&self) -> bool {
        matches!(self, Health::Error(_) | Health::Blocked { .. })
    }

    /// 種類の名前。
    pub(crate) fn title(&self) -> &'static str {
        match self {
            Health::Warning(_) => "注意",
            Health::Error(_) => "エラー",
            Health::Blocked { .. } => "計算していない",
            Health::Ok => "計算できた",
            Health::Idle => "",
        }
    }

    /// 知らせの文。
    pub(crate) fn message(&self) -> String {
        match self {
            Health::Warning(m) | Health::Error(m) => m.clone(),
            Health::Blocked { by, .. } => {
                format!("{by} が使えないので、この機能は計算していません")
            }
            _ => String::new(),
        }
    }

    /// 印の色。
    pub(crate) fn color(&self, l: &theme::Look) -> egui::Color32 {
        match self {
            Health::Ok => l.pal.ok,
            Health::Warning(_) => l.pal.warn,
            Health::Error(_) | Health::Blocked { .. } => l.pal.err,
            Health::Idle => l.pal.ink3,
        }
    }
}

/// 具合の印を `c` を中心に描く。計算できた物は小さな点、注意は三角に「!」、エラーは丸に「×」、
/// 止まっている物は輪(形で見分けられるので、色が見分けにくくても分かる)。
pub(crate) fn paint_health(
    p: &egui::Painter,
    c: egui::Pos2,
    h: &Health,
    l: &theme::Look,
    dim: bool,
) {
    let fade = |k: egui::Color32| if dim { k.gamma_multiply(0.4) } else { k };
    let col = fade(h.color(l));
    let paper = fade(l.pal.panel);
    match h {
        Health::Ok => {
            p.circle_filled(c, 3.0, col);
        }
        Health::Warning(_) => {
            let r = 5.5;
            p.add(egui::Shape::convex_polygon(
                vec![
                    c + Vec2::new(0.0, -r),
                    c + Vec2::new(r * 0.95, r * 0.75),
                    c + Vec2::new(-r * 0.95, r * 0.75),
                ],
                col,
                Stroke::NONE,
            ));
            p.line_segment(
                [c + Vec2::new(0.0, -2.2), c + Vec2::new(0.0, 1.2)],
                Stroke::new(1.3, paper),
            );
            p.circle_filled(c + Vec2::new(0.0, 2.9), 0.7, paper);
        }
        Health::Error(_) => {
            p.circle_filled(c, 5.0, col);
            let d = 2.2;
            for (a, b) in [
                (Vec2::new(-d, -d), Vec2::new(d, d)),
                (Vec2::new(-d, d), Vec2::new(d, -d)),
            ] {
                p.line_segment([c + a, c + b], Stroke::new(1.4, paper));
            }
        }
        Health::Blocked { .. } => {
            p.circle_stroke(c, 3.6, Stroke::new(1.6, col));
        }
        Health::Idle => {}
    }
}

/// いちばん重い問題(エラー > 止まっている > 注意)。問題が無ければ `None`。
pub(crate) fn worst_health(all: impl Iterator<Item = Health>) -> Option<Health> {
    all.filter(Health::is_problem).max_by_key(|h| match h {
        Health::Error(_) => 2,
        Health::Blocked { .. } => 1,
        _ => 0,
    })
}

/// 指を置いた時の説明(名前、種類と文、確かめ方)。
pub(crate) fn health_tip(name: &str, h: &Health) -> String {
    if !h.is_problem() {
        return name.to_string();
    }
    format!(
        "{name}\n{}: {}\n右クリックの「{}を確かめる」で詳しく、「文を写す」で写せる",
        h.title(),
        h.message(),
        if h.is_red() { "エラー" } else { "注意" }
    )
}

/// 計算にかかった時間の書き方(タイムラインの吹き出し)。
pub(crate) fn time_text(ms: f64) -> String {
    if ms >= 1000.0 {
        format!("{:.1} 秒", ms / 1000.0)
    } else if ms >= 10.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{ms:.1} ms")
    }
}

/// タイムラインの札の吹き出し: 名前と具合、計算にかかった時間、操作のヒント(
/// 重い機能を探せ、ダブルクリック・ドラッグ・右クリックでできることを使いながら覚えられる)。
pub(crate) fn chip_tip(name: &str, h: &Health, ms: Option<f64>, reused: bool) -> String {
    let mut s = health_tip(name, h);
    if let Some(ms) = ms {
        s.push_str(&format!("\n計算 {}", time_text(ms)));
        if reused {
            s.push_str("(前の結果を使い回し)");
        }
    }
    s.push_str("\nダブルクリックで編集・ドラッグで並べ替え・右クリックでほかの操作");
    s
}

/// 並べ替えのドラッグ(つかんだ機能)。
#[derive(Clone, Copy)]
struct Dragging(FeatureId);

/// 今の位置の印をつかんで引いている途中。
#[derive(Clone, Copy)]
struct MarkerDrag;

/// 縦の一覧の今の位置の印(左に五角の取っ手、右へ細い棒)。
/// 下の帯(タイムライン・段の帯・ストーリーボード)の枠の高さ。字とアイコンの倍率に合わせる
/// (1 倍で 60)。固定の高さにすると、250 % で倍率を掛けた札と字が枠に入らない。
pub(crate) fn strip_height() -> f32 {
    60.0 * theme::band()
}

/// 下の帯の左のボタンと枠の間、枠と右の欄の間。
pub(crate) const STRIP_GAP: f32 = 10.0;

/// 下の帯の枠(3 つの帯で同じ地・縁・角)。
pub(crate) fn paint_strip(ui: &egui::Ui, rect: egui::Rect, l: &theme::Look) {
    ui.painter().rect(
        rect,
        theme::corner(6.0),
        l.pal.panel,
        Stroke::new(1.0, l.pal.edge),
        egui::StrokeKind::Inside,
    );
}

/// 横のタイムラインの札の大きさ(1 倍で 36×34)。アイコンの倍率で広げる。
/// 札を固定の大きさにすると、倍率を掛けたアイコンが隣の札へはみ出す。
fn chip_size() -> Vec2 {
    let icon = theme::icon_px(18.0);
    Vec2::new(icon + 18.0, icon + 16.0)
}

/// 横のタイムラインの札の下の、グループの下線の分の高さ(線と、札との間)。
fn group_line_zone() -> f32 {
    3.0 + 2.0 * theme::band().sqrt()
}

/// 横のタイムラインの名前の 1 行の高さ(道具の名前、グループの名前)。
fn name_row() -> f32 {
    theme::fs(10.5) * 1.25 + 2.0
}

/// 横のスクロールバーの分(札に重ねず、札の下に出す)。
fn scroll_bar_zone() -> f32 {
    let s = egui::style::ScrollStyle::solid();
    s.bar_width + s.bar_inner_margin + s.bar_outer_margin
}

/// 横の帯の、札の周りの余白(上下)。
const STRIP_PAD_Y: f32 = 3.0;

/// 横のタイムラインの帯の最小の高さ(アイコンの札・グループの下線・スクロールバーだけが入る)。
fn min_strip_height() -> f32 {
    chip_size().y + group_line_zone() + scroll_bar_zone() + 2.0 * STRIP_PAD_Y
}

/// 設計のタイムラインの部品の既定の高さ(今出ている物、アイコンの札が入る最小。2026-10-21 Kie)。
/// 加工の段の帯と同じ 72(字の倍率を掛ける)にすると、札の上下に空きが出る。
pub(crate) fn band_min_height() -> f32 {
    min_strip_height() + 4.0
}

/// 横のタイムラインの行の作り(帯の高さで決める)。最小はアイコンだけ。広げると 1 行目に道具の
/// 名前を札の下へ、2 行目にグループの名前を札の上へ足す(2026-10-21 Kie)。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Rows {
    /// 札の上の、グループの名前の行(出さなければ 0)
    pub top: f32,
    /// 札の下の、道具の名前の行(出さなければ 0)
    pub name: f32,
}

impl Rows {
    /// 帯の中の高さ `h` に入る行。グループが無ければ、グループの名前の行は取らない。
    pub(crate) fn fit(h: f32, min: f32, groups: bool) -> Rows {
        let row = name_row();
        let extra = ((h - min) / row).floor().max(0.0) as usize;
        Rows {
            name: if extra >= 1 { row } else { 0.0 },
            top: if groups && extra >= 2 { row } else { 0.0 },
        }
    }

    /// 札 1 つの高さ(グループの名前の行・札・道具の名前の行・グループの下線)。
    fn item_h(&self) -> f32 {
        self.top + chip_size().y + self.name + group_line_zone()
    }
}

/// 横のタイムラインのグループの、並べた後に名前を書く所(外のグループから順)。
struct GroupMark {
    id: u64,
    name: String,
    color: egui::Color32,
    /// 札とグループの頭の印を並べた所の左右
    span: egui::Rangef,
    /// 並べた所の上の端(名前の行の上)
    top: f32,
}

fn paint_marker_v(p: &egui::Painter, y: f32, xs: egui::Rangef, color: egui::Color32) {
    let left = xs.min;
    let h = 5.0;
    p.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(left, y - h),
            egui::pos2(left + 7.0, y - h),
            egui::pos2(left + 11.0, y),
            egui::pos2(left + 7.0, y + h),
            egui::pos2(left, y + h),
        ],
        color,
        Stroke::NONE,
    ));
    p.rect_filled(
        Rect::from_x_y_ranges(
            egui::Rangef::new(left + 8.0, xs.max),
            egui::Rangef::new(y - 1.0, y + 1.0),
        ),
        1.0,
        color,
    );
}

/// 引いている所の、並べる向きの位置(横のタイムラインは x、縦の一覧は y)。
fn along(vertical: bool, p: egui::Pos2) -> f32 {
    if vertical { p.y } else { p.x }
}

/// 機能を `p` で離した時に落ちる所の線(両端)。横の並びは機能の左か右の縁に縦の線、縦の一覧は
/// 上か下の縁に横の線。落とす先の決め方は離した時(`finish_drag`)と同じで、機能の後ろ半分なら後ろへ。
fn drop_line(
    vertical: bool,
    chips: &[(FeatureId, egui::Rect)],
    p: egui::Pos2,
) -> Option<(egui::Pos2, egui::Pos2)> {
    let (_, r) = chips.iter().find(|(_, r)| r.contains(p))?;
    let after = along(vertical, p) > along(vertical, r.center());
    Some(if vertical {
        let y = if after { r.bottom() } else { r.top() };
        (egui::pos2(r.left(), y), egui::pos2(r.right(), y))
    } else {
        let x = if after { r.right() } else { r.left() };
        (egui::pos2(x, r.top()), egui::pos2(x, r.bottom()))
    })
}

/// 今の位置の印を描く(下へ細い棒)。
fn paint_marker(p: &egui::Painter, x: f32, ys: egui::Rangef, color: egui::Color32) {
    let top = ys.min;
    let w = 5.0;
    p.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(x - w, top),
            egui::pos2(x + w, top),
            egui::pos2(x + w, top + 7.0),
            egui::pos2(x, top + 11.0),
            egui::pos2(x - w, top + 7.0),
        ],
        color,
        Stroke::NONE,
    ));
    p.rect_filled(
        Rect::from_x_y_ranges(
            egui::Rangef::new(x - 1.0, x + 1.0),
            egui::Rangef::new(top + 8.0, ys.max),
        ),
        1.0,
        color,
    );
}

impl Workspace {
    /// 下のタイムライン(試作の `.timeline`): 左に戻す・進めるのボタン、中に機能のアイコンの札
    /// (グループは色の下線)、右に今の様子。部品の高さを広げると、道具とグループの名前を足す。
    pub(crate) fn timeline_ui(&mut self, ui: &mut egui::Ui) {
        // 縦長の所(左右に置いた時など)は縦の一覧にする
        self.win.timeline_vertical = ui.available_height() > ui.available_width();
        if self.win.timeline_vertical {
            self.timeline_list(ui);
            return;
        }
        let l = theme::look(ui.ctx());
        let data = self.cur.work.doc().data().clone();
        let mut chips: Vec<(FeatureId, egui::Rect)> = Vec::new();
        // 帯は部品の高さいっぱいに取る(最小はアイコンの札が入る高さ)。広げた分で名前の行を足す
        let min = min_strip_height();
        let strip_h = (ui.available_height() - 4.0).max(min);
        let groups = data.groups.iter().any(|g| {
            data.group_range(g).is_some_and(|(s, e)| {
                !data.features[s..=e]
                    .iter()
                    .all(|f| self.is_cam_feature(&f.type_id))
            })
        });
        let rows = Rows::fit(strip_h, min, groups);
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            self.rollback_buttons(ui, &data);
            ui.add_space(STRIP_GAP);
            let info_w = theme::fs(200.0).min(ui.available_width() * 0.2);
            let w = (ui.available_width() - info_w - STRIP_GAP).max(120.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, strip_h), Sense::hover());
            paint_strip(ui, rect, &l);
            // 札の行(とスクロールバー)は帯の上下の真ん中に置き、スクロールバーは札に重ねず札の下に出す
            let used = rows.item_h() + scroll_bar_zone();
            let top = rect.top() + ((rect.height() - used) / 2.0).max(STRIP_PAD_Y);
            let inner = Rect::from_min_max(
                egui::pos2(rect.left() + 8.0, top),
                egui::pos2(rect.right() - 8.0, (top + used).min(rect.bottom())),
            );
            let mut strip = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(inner)
                    .layout(egui::Layout::left_to_right(egui::Align::Min)),
            );
            strip.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            // 横にしか送れない帯なので、縦のホイールでも横へ送る。egui は
            // 既定で縦のホイールを横の送りに使わない(Style::always_scroll_the_only_direction が偽)
            strip.style_mut().always_scroll_the_only_direction = true;
            egui::ScrollArea::horizontal()
                .id_salt("timeline_scroll")
                .show(&mut strip, |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        if data.features.is_empty() {
                            ui.allocate_ui_with_layout(
                                Vec2::new(ui.available_width(), rows.item_h()),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            "機能はまだありません。上のツールバーから作ります",
                                        )
                                        .color(l.pal.ink3),
                                    );
                                },
                            );
                        }
                        let mut marks = Vec::new();
                        self.timeline_range(
                            ui,
                            &data,
                            (0, data.features.len()),
                            &[],
                            rows,
                            &mut chips,
                            &mut marks,
                        );
                        self.group_names(ui, &data, rows, &marks);
                    });
                });
            self.finish_marker_drag(ui, &data, &chips, rect);
            ui.add_space(STRIP_GAP);
            // 右の様子の欄は、札の帯と同じ高さの中の上下の真ん中に置く(帯の上に寄せない)
            let lines = self.timeline_info(&data).len().max(1) as f32;
            let row = ui.fonts_mut(|f| f.row_height(&theme::prop(11.0)));
            let used = lines * row + (lines - 1.0) * 2.0;
            ui.allocate_ui_with_layout(
                Vec2::new(info_w, rect.height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(info_w);
                    ui.add_space(((rect.height() - used) / 2.0).max(0.0));
                    self.timeline_info_lines(ui, &data);
                },
            );
        });
        self.finish_drag(ui, &data, &chips);
    }

    /// 縦の一覧(上に戻す・進めるのボタン、中に上から古い順の行、下に今の様子)。
    fn timeline_list(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        let data = self.cur.work.doc().data().clone();
        let mut chips: Vec<(FeatureId, egui::Rect)> = Vec::new();
        let full = ui.available_rect_before_wrap();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            self.rollback_buttons(ui, &data);
        });
        ui.add_space(4.0);
        // 下の様子の分を残して、残りを一覧にする
        let info_h = 3.0 * (theme::fs(11.0) + 4.0) + 8.0;
        let list_h = (ui.available_height() - info_h).max(60.0);
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), list_h), Sense::hover());
        ui.painter().rect(
            rect,
            6.0,
            l.pal.panel,
            Stroke::new(1.0, l.pal.edge),
            egui::StrokeKind::Inside,
        );
        let mut list = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink(4.0))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        list.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        egui::ScrollArea::vertical()
            .id_salt("timeline_list_scroll")
            .auto_shrink([false, false])
            .show(&mut list, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                if data.features.is_empty() {
                    ui.label(
                        egui::RichText::new("機能はまだありません。上のツールバーから作ります")
                            .color(l.pal.ink3),
                    );
                }
                self.timeline_range(
                    ui,
                    &data,
                    (0, data.features.len()),
                    &[],
                    Rows::default(),
                    &mut chips,
                    &mut Vec::new(),
                );
            });
        self.finish_marker_drag(ui, &data, &chips, rect);
        ui.add_space(4.0);
        ui.allocate_ui(Vec2::new(full.width(), info_h - 8.0), |ui| {
            self.timeline_info_lines(ui, &data);
        });
        self.finish_drag(ui, &data, &chips);
    }

    /// ロールバックの棒を先頭・1 つ前・1 つ後ろ・最後へ動かすボタン。
    fn rollback_buttons(&mut self, ui: &mut egui::Ui, data: &kmb_core::DocumentData) {
        let n = data.features.len() as i32;
        for (icon, name, step) in [
            ("prev", "ロールバックの棒を先頭へ", -n),
            (
                "back",
                "ロールバックの棒を 1 つ前へ(後ろの機能を一時的に無効にする)",
                -1,
            ),
            ("fwd", "ロールバックの棒を 1 つ後ろへ", 1),
            ("next", "ロールバックの棒を最後へ(すべて有効にする)", n),
        ] {
            if theme::icon_button(ui, icon, name, n > 0, false)
                .on_hover_text(name)
                .clicked()
            {
                // 加工の機能はこのタイムラインに出さないので、設計の機能ごとに動かす
                let now = data.active_len() as i32;
                let to = self.rollback_target(data, step) as i32;
                self.step_rollback(to - now);
            }
        }
    }

    /// 今の様子の行(長い注意は 1 行で切り、全文は指を置くと出す)。
    fn timeline_info_lines(&self, ui: &mut egui::Ui, data: &kmb_core::DocumentData) {
        let l = theme::look(ui.ctx());
        ui.spacing_mut().item_spacing.y = 2.0;
        for line in self.timeline_info(data) {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&line)
                        .size(crate::theme::fs(11.0))
                        .color(l.pal.ink2),
                )
                .truncate(),
            )
            .on_hover_text(&line);
        }
    }

    /// タイムラインの右に出す様子(選んでいればその機能、なければ全体)。
    fn timeline_info(&self, data: &kmb_core::DocumentData) -> Vec<String> {
        if let Some(id) = self.cur.selected
            && let Some(f) = data.feature(id)
        {
            let group = data
                .groups
                .iter()
                .filter(|g| edit::group_members(data, g.id).contains(&id))
                .min_by_key(|g| edit::group_members(data, g.id).len())
                .map(|g| format!(" ・ {}", g.name))
                .unwrap_or_default();
            let (_, detail) = self.status_line(id);
            let mut v = vec![format!("{}{group}", f.name)];
            if !detail.is_empty() {
                v.push(detail.lines().next().unwrap_or("").to_string());
            }
            return v;
        }
        // 設計のタイムラインなので、加工の機能は数えない
        let design: Vec<&kmb_core::FeatureEntry> = data
            .features
            .iter()
            .filter(|f| !self.is_cam_feature(&f.type_id))
            .collect();
        let rolled = data.features[data.active_len()..]
            .iter()
            .filter(|f| !self.is_cam_feature(&f.type_id))
            .count();
        let mut s = format!("{} 機能", design.len());
        let (mut errors, mut warnings) = (0, 0);
        for f in design {
            match Health::of(self.cur.work.report().feature(f.id)) {
                Health::Error(_) | Health::Blocked { .. } => errors += 1,
                Health::Warning(_) => warnings += 1,
                _ => {}
            }
        }
        if errors > 0 {
            s += &format!(" ・ エラー {errors}");
        }
        if warnings > 0 {
            s += &format!(" ・ 注意 {warnings}");
        }
        if !data.groups.is_empty() {
            s += &format!(" ・ グループ {}", data.groups.len());
        }
        if rolled > 0 {
            s += &format!(" ・ {rolled} つはロールバックの後");
        }
        vec![s]
    }

    /// 加工の機能(セットアップ・加工・工具)と解析の研究か。設計のタイムラインには出さない
    /// (それぞれの作業空間のブラウザに出す)。
    pub(crate) fn is_cam_feature(&self, type_id: &str) -> bool {
        self.win.registry.descriptor(type_id).is_some_and(|d| {
            matches!(
                d.category.as_str(),
                kmb_core::category::OPERATION
                    | kmb_core::category::SETUP
                    | kmb_core::category::TOOLS
                    | kmb_core::category::STUDY
            )
        })
    }

    /// 戻す・進めるのボタンの行き先(ロールバックの位置 = 有効な機能の数)。
    /// 加工の機能は飛ばし、設計の機能を 1 つずつ動かす。`step` が機能の数以上なら端まで。
    pub(crate) fn rollback_target(&self, data: &kmb_core::DocumentData, step: i32) -> usize {
        let n = data.features.len();
        let now = data.active_len();
        let design = |i: usize| !self.is_cam_feature(&data.features[i].type_id);
        if step.unsigned_abs() as usize >= n.max(1) {
            return if step < 0 { 0 } else { n };
        }
        if step < 0 {
            // いちばん後ろの有効な設計の機能を無効にする
            (0..now).rev().find(|i| design(*i)).unwrap_or(now)
        } else {
            match (now..n).find(|i| design(*i)) {
                // その後ろに設計の機能が無ければ最後まで(後ろの加工も有効にする)
                Some(i) if (i + 1..n).any(design) => i + 1,
                _ => n,
            }
        }
    }

    /// 今の位置の印(いつも出す。ここより後ろはロールバックで一時的に無効)。つかんで左右へ引ける。
    fn marker(&mut self, ui: &mut egui::Ui, h: f32) {
        let l = theme::look(ui.ctx());
        let v = self.win.timeline_vertical;
        let size = if v {
            Vec2::new(ui.available_width(), 12.0)
        } else {
            Vec2::new(12.0, h)
        };
        let (rect, r) = ui.allocate_exact_size(size, Sense::click_and_drag());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "今の位置の印"));
        let marker = crate::doc_id(ui.ctx(), "timeline_marker");
        let dragging = ui
            .ctx()
            .data(|d| d.get_temp::<MarkerDrag>(marker))
            .is_some();
        let c = if dragging {
            l.pal.ink3
        } else if r.hovered() {
            l.ws()
        } else {
            l.pal.ink2
        };
        if v {
            paint_marker_v(ui.painter(), rect.center().y, rect.x_range(), c);
        } else {
            paint_marker(ui.painter(), rect.center().x, rect.y_range(), c);
        }
        if r.drag_started() {
            ui.ctx().data_mut(|d| d.insert_temp(marker, MarkerDrag));
        }
        if v {
            r.on_hover_cursor(egui::CursorIcon::ResizeVertical)
                .on_hover_text(
                    "今の位置: ここまでの機能で形を作っている。上下へ引くと、そこまで戻す・進める\n\
                     (下の機能は消えない。上の矢印のボタンでも動かせる)",
                );
        } else {
            r.on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                .on_hover_text(
                    "今の位置: ここまでの機能で形を作っている。左右へ引くと、そこまで戻す・進める\n\
                     (後ろの機能は消えない。左の矢印のボタンでも動かせる)",
                );
        }
    }

    /// 今の位置の印を引いている間は行き先に印の影を出し、離したらそこまで戻す・進める。
    fn finish_marker_drag(
        &mut self,
        ui: &egui::Ui,
        data: &kmb_core::DocumentData,
        chips: &[(FeatureId, egui::Rect)],
        strip: Rect,
    ) {
        let key = crate::doc_id(ui.ctx(), "timeline_marker");
        if ui.ctx().data(|d| d.get_temp::<MarkerDrag>(key)).is_none() {
            return;
        }
        let Some(p) = ui.input(|i| i.pointer.latest_pos()) else {
            return;
        };
        // 離した所より前(左・上)にある、いちばん後ろの札の後ろ(無ければ先頭)
        let v = self.win.timeline_vertical;
        let at = along(v, p);
        let before = chips
            .iter()
            .filter(|(_, r)| along(v, r.center()) < at)
            .max_by(|a, b| along(v, a.1.center()).total_cmp(&along(v, b.1.center())));
        let l = theme::look(ui.ctx());
        if v {
            let y = match before {
                Some((_, r)) => r.bottom() + 3.0,
                None => {
                    chips
                        .iter()
                        .map(|(_, r)| r.top())
                        .fold(strip.top() + 8.0, f32::min)
                        - 3.0
                }
            };
            paint_marker_v(ui.painter(), y, strip.shrink(4.0).x_range(), l.ws());
        } else {
            let x = match before {
                Some((_, r)) => r.right() + 4.0,
                None => {
                    chips
                        .iter()
                        .map(|(_, r)| r.left())
                        .fold(strip.left() + 10.0, f32::min)
                        - 4.0
                }
            };
            let ys = chips
                .first()
                .map_or(strip.shrink(8.0).y_range(), |(_, r)| r.y_range());
            paint_marker(ui.painter(), x, ys, l.ws());
        }
        if ui.input(|i| i.pointer.any_down()) {
            return;
        }
        ui.ctx().data_mut(|d| d.remove::<MarkerDrag>(key));
        let n = data.features.len();
        let to = match before.and_then(|(id, _)| data.index_of(*id)) {
            Some(i) => {
                let rest_design =
                    (i + 1..n).any(|j| !self.is_cam_feature(&data.features[j].type_id));
                if rest_design { i + 1 } else { n }
            }
            None => 0,
        };
        let now = data.active_len();
        if to != now {
            log::info!("タイムラインの今の位置の印を {now} から {to} へ動かす");
            self.step_rollback(to as i32 - now as i32);
        }
    }

    /// `a..b` の機能を並べる。グループの始まりなら、横は色の下線、縦は左の色の縦線で示す
    /// (`outer` はいま中にいるグループ)。グループは入れ子か離れているかのどちらか(コアが確かめる)
    /// なので、範囲の中で始まるものは中に収まる。加工の機能は出さない(中身が加工だけのグループも
    /// 出さない)。`marks` には横のグループの名前を書く所を足す。
    #[allow(clippy::too_many_arguments)]
    fn timeline_range(
        &mut self,
        ui: &mut egui::Ui,
        data: &kmb_core::DocumentData,
        (a, b): (usize, usize),
        outer: &[u64],
        rows: Rows,
        chips: &mut Vec<(FeatureId, egui::Rect)>,
        marks: &mut Vec<GroupMark>,
    ) {
        let mut i = a;
        let marker_h = rows.item_h();
        // 全部を戻した時は、印を先頭に出す
        if a == 0 && outer.is_empty() && b > 0 && data.active_len() == 0 {
            self.marker(ui, marker_h);
        }
        while i < b {
            // i で始まり b を越えないグループのうち、いちばん外側のもの
            let next = data
                .groups
                .iter()
                .filter(|g| !outer.contains(&g.id))
                .filter_map(|g| data.group_range(g).map(|r| (g, r)))
                .filter(|(_, (s, e))| *s == i && *e < b)
                .max_by_key(|(g, (s, e))| (e - s, std::cmp::Reverse(g.id)));
            match next {
                Some((g, (s, e))) => {
                    let g = g.clone();
                    let all_cam = data.features[s..=e]
                        .iter()
                        .all(|f| self.is_cam_feature(&f.type_id));
                    if !all_cam {
                        let mut path = outer.to_vec();
                        path.push(g.id);
                        if self.win.timeline_vertical {
                            self.group_rows(ui, data, &g, (s, e), &path, chips);
                        } else {
                            self.group_span(ui, data, &g, (s, e), &path, rows, chips, marks);
                        }
                    }
                    i = e + 1;
                }
                None => {
                    if !self.is_cam_feature(&data.features[i].type_id) {
                        self.chip(ui, data, i, rows, chips);
                    }
                    i += 1;
                }
            }
            // 今の位置の印(この機能の後ろが今の位置なら。最後まで有効な時も最後に出す)。
            // グループの終わりの位置は、グループの外で出す(二重に出さない)
            if i == data.active_len() && (i < b || outer.is_empty()) {
                self.marker(ui, marker_h);
            }
        }
    }

    /// グループの頭の開閉の印(横も縦も同じ)。押すと畳む・開く、右クリックでグループのメニュー。
    fn group_toggle(&mut self, ui: &mut egui::Ui, g: &Group, tw: Rect, r: egui::Response) {
        let l = theme::look(ui.ctx());
        let color = group_color(g.id, &l);
        let collapsed = self.cur.collapsed.contains(&g.id);
        let verb = if collapsed { "開く" } else { "畳む" };
        r.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("グループ「{}」を{verb}", g.name),
            )
        });
        if r.hovered() {
            ui.painter().rect_filled(
                tw,
                crate::theme::corner(3.0),
                theme::mix(l.pal.panel, color, 0.18),
            );
        }
        icons::paint_exact(
            ui.painter(),
            tw.shrink(2.0),
            if collapsed { "chevr" } else { "chev" },
            color,
            1.8,
        );
        let n = edit::group_members(self.cur.work.doc().data(), g.id).len();
        let r = r.on_hover_text(format!(
            "グループ「{}」({n} 個)\n押すと{verb}。右クリックで名前変更・解く・消す",
            g.name
        ));
        if r.clicked() {
            if collapsed {
                self.cur.collapsed.remove(&g.id);
            } else {
                self.cur.collapsed.insert(g.id);
            }
        }
        r.context_menu(|ui| self.group_menu(ui, g));
    }

    /// グループの名前を打つ欄(グループを作った直後と「名前変更」)。Enter・欄の外を押す・注目が
    /// 外れると決め、Esc で取り消す。
    ///
    /// 注目を移すのは打ち始めの 1 こまだけにする。毎こま入力欄へ注目を戻す(`request_focus`)と、
    /// Enter・Esc・外を押すで注目が外れても同じこまで戻り、`lost_focus` が一度も立たず抜けられない
    /// (2026-10-21 Kie の報告)。
    fn group_rename_box(&mut self, ui: &mut egui::Ui, gid: u64, at: Rect) {
        let Some((_, name)) = self.cur.renaming_group.as_mut() else {
            return;
        };
        // 打ち始めたグループ(別のグループの名前を打ち始めたら、また 1 こまだけ注目を移す)
        let started = crate::doc_id(ui.ctx(), "timeline_group_rename");
        let r = ui.put(
            at,
            egui::TextEdit::singleline(name)
                .id_salt((started, gid))
                .desired_width(at.width())
                .font(crate::theme::prop(11.0)),
        );
        // 台本と読み上げが名前で見つけられるように
        ui.ctx()
            .accesskit_node_builder(r.id, |b| b.set_label("グループの名前"));
        let first = ui.ctx().data_mut(|d| {
            let first = d.get_temp::<u64>(started) != Some(gid);
            d.insert_temp(started, gid);
            first
        });
        if first {
            r.request_focus();
            return;
        }
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let done = if r.lost_focus() || !r.has_focus() {
            Some(!esc)
        } else {
            None
        };
        let Some(ok) = done else {
            return;
        };
        ui.ctx().data_mut(|d| d.remove::<u64>(started));
        let Some((rid, name)) = self.cur.renaming_group.take() else {
            return;
        };
        if ok {
            if let Ok(tx) = edit::rename_group(self.cur.work.doc().data(), rid, &name) {
                log::info!("グループの名前を「{name}」にする");
                self.apply(tx);
            }
        } else {
            log::info!("グループの名前の変更をやめる");
        }
    }

    /// 横のタイムラインのグループ: 頭に開閉の印、続けて中身の札、下に色の下線(2026-10-21 Kie:
    /// 枠で囲むと場所を取るので下線にした)。名前は広げた時に札の上へ書く(`group_names`)。
    #[allow(clippy::too_many_arguments)]
    fn group_span(
        &mut self,
        ui: &mut egui::Ui,
        data: &kmb_core::DocumentData,
        g: &Group,
        (s, e): (usize, usize),
        path: &[u64],
        rows: Rows,
        chips: &mut Vec<(FeatureId, egui::Rect)>,
        marks: &mut Vec<GroupMark>,
    ) {
        let l = theme::look(ui.ctx());
        let color = group_color(g.id, &l);
        let collapsed = self.cur.collapsed.contains(&g.id);
        let members = edit::group_members(data, g.id);
        let has_sel = self.selection().iter().any(|id| members.contains(id));
        let item_h = rows.item_h();
        let chip = chip_size();
        // 線は中身より先に描く所を取っておく(入れ子の内のグループの線が上に重なる)
        let slot = ui.painter().add(egui::Shape::Noop);
        let resp = ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let icon = theme::icon_px(14.0);
            let (head, tr) = ui.allocate_exact_size(Vec2::new(icon + 2.0, item_h), Sense::click());
            let tw = Rect::from_center_size(
                egui::pos2(head.center().x, head.top() + rows.top + chip.y / 2.0),
                Vec2::splat(icon),
            );
            self.group_toggle(ui, g, tw, tr);
            if self
                .cur
                .renaming_group
                .as_ref()
                .is_some_and(|(id, _)| *id == g.id)
            {
                let (r, _) =
                    ui.allocate_exact_size(Vec2::new(theme::fs(96.0), item_h), Sense::hover());
                let h = theme::fs(11.0) * 1.25 + 6.0;
                let at = Rect::from_center_size(
                    egui::pos2(r.center().x, r.top() + rows.top + chip.y / 2.0),
                    Vec2::new(r.width(), h),
                );
                self.group_rename_box(ui, g.id, at);
            }
            if collapsed {
                // 畳んでも、中身のアイコンを 3 つまでと残りの数を出す
                let small = theme::icon_px(15.0);
                let n = e - s + 1;
                let more = (n > 3).then(|| format!("+{}", n - 3));
                let more_w = more.as_ref().map_or(0.0, |t| {
                    ui.painter()
                        .layout_no_wrap(t.clone(), theme::prop(10.5), l.pal.ink3)
                        .size()
                        .x
                        + 3.0
                });
                let w = n.min(3) as f32 * (small + 3.0) + more_w + 14.0;
                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, item_h), Sense::hover());
                let cy = r.top() + rows.top + chip.y / 2.0;
                let mut x = r.left() + 2.0;
                for f in data.features[s..=e].iter().take(3) {
                    let icon = self.feature_icon_name(&f.type_id);
                    let ir =
                        Rect::from_min_size(egui::pos2(x, cy - small / 2.0), Vec2::splat(small));
                    icons::paint_exact(ui.painter(), ir, &icon, l.pal.ink2, 1.6);
                    x += small + 3.0;
                }
                if let Some(t) = more {
                    ui.painter().text(
                        egui::pos2(x, cy),
                        egui::Align2::LEFT_CENTER,
                        t,
                        theme::prop(10.5),
                        l.pal.ink3,
                    );
                }
                // 畳んでいても、中に問題があれば印を出す(エラーがあればエラーの印)
                let worst = worst_health(
                    data.features[s..=e]
                        .iter()
                        .map(|f| Health::of(self.cur.work.report().feature(f.id))),
                );
                if let Some(h) = worst {
                    paint_health(ui.painter(), egui::pos2(r.right() - 6.0, cy), &h, &l, false);
                    resp.on_hover_text(format!("中に{}があります(開くと見える)", h.title()));
                }
            } else {
                let mut inner = Vec::new();
                self.timeline_range(ui, data, (s, e + 1), path, rows, chips, &mut inner);
                marks.extend(inner);
            }
        });
        let span = resp.response.rect;
        let t = 1.5 + theme::band().sqrt();
        let y = span.bottom() - group_line_zone() / 2.0;
        let line = Rect::from_x_y_ranges(
            egui::Rangef::new(span.left() + 1.0, span.right() - 1.0),
            egui::Rangef::new(y - t / 2.0, y + t / 2.0),
        );
        let mut shapes = Vec::new();
        if has_sel {
            // 中身を選んでいる時は、札の所をグループの色で薄く塗る
            let band = Rect::from_x_y_ranges(
                span.x_range(),
                egui::Rangef::new(span.top() + rows.top, y - t / 2.0 - 1.0),
            );
            shapes.push(egui::Shape::rect_filled(
                band,
                crate::theme::corner(4.0),
                color.gamma_multiply(0.14),
            ));
        }
        shapes.push(egui::Shape::rect_filled(line, t / 2.0, color));
        ui.painter().set(slot, egui::Shape::Vec(shapes));
        marks.push(GroupMark {
            id: g.id,
            name: g.name.clone(),
            color,
            span: span.x_range(),
            top: span.top(),
        });
    }

    /// 横のタイムラインのグループの名前を、札の上の行に書く(広げて行がある時だけ)。入れ子で頭が
    /// 近い時は、前の名前の後ろへずらす。押すと中身をまとめて選び、右クリックでグループのメニュー。
    fn group_names(
        &mut self,
        ui: &mut egui::Ui,
        data: &kmb_core::DocumentData,
        rows: Rows,
        marks: &[GroupMark],
    ) {
        if rows.top <= 0.0 {
            return;
        }
        let mut marks: Vec<&GroupMark> = marks.iter().collect();
        marks.sort_by(|a, b| {
            a.span
                .min
                .total_cmp(&b.span.min)
                .then(b.span.span().total_cmp(&a.span.span()))
        });
        let mut cursor = f32::MIN;
        for m in marks {
            if self
                .cur
                .renaming_group
                .as_ref()
                .is_some_and(|(id, _)| *id == m.id)
            {
                continue;
            }
            let x = (m.span.min + 2.0).max(cursor);
            let max_w = m.span.max - x;
            if max_w < theme::fs(12.0) {
                continue;
            }
            let mut job = egui::text::LayoutJob::simple_singleline(
                m.name.clone(),
                theme::prop(10.5),
                m.color,
            );
            job.wrap = egui::text::TextWrapping {
                max_width: max_w,
                max_rows: 1,
                break_anywhere: true,
                overflow_character: Some('…'),
            };
            let galley = ui.painter().layout_job(job);
            let rect =
                Rect::from_min_size(egui::pos2(x, m.top), Vec2::new(galley.size().x, rows.top));
            ui.painter().galley(
                egui::pos2(x, rect.center().y - galley.size().y / 2.0),
                galley,
                m.color,
            );
            cursor = rect.right() + 6.0;
            let r = ui.interact(
                rect,
                egui::Id::new(("timeline_group_name", m.id)),
                Sense::click(),
            );
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &m.name));
            let r = r.on_hover_text("押すと中身をまとめて選ぶ。右クリックで名前・解く・消す");
            if r.clicked() {
                self.select_group_members(data, m.id);
            }
            if let Some(g) = data.groups.iter().find(|g| g.id == m.id).cloned() {
                r.context_menu(|ui| self.group_menu(ui, &g));
            }
        }
    }

    /// グループの中身をまとめて選ぶ。
    fn select_group_members(&mut self, data: &kmb_core::DocumentData, gid: u64) {
        let ids = edit::group_members(data, gid);
        if let Some(first) = ids.first() {
            self.select(*first);
            for id in &ids[1..] {
                self.toggle_select(*id);
            }
        }
    }

    /// 縦の一覧のグループ: 見出しの行(開閉の印・名前・数)の下に、中身の行を少し下げて並べ、
    /// 左に色の縦線を引く(2026-10-21 Kie: 枠で囲まない)。
    fn group_rows(
        &mut self,
        ui: &mut egui::Ui,
        data: &kmb_core::DocumentData,
        g: &Group,
        (s, e): (usize, usize),
        path: &[u64],
        chips: &mut Vec<(FeatureId, egui::Rect)>,
    ) {
        let l = theme::look(ui.ctx());
        let color = group_color(g.id, &l);
        let collapsed = self.cur.collapsed.contains(&g.id);
        let members = edit::group_members(data, g.id);
        let has_sel = self.selection().iter().any(|id| members.contains(id));
        let row_h = 22.0_f32.max(theme::fs(10.5) + 8.0);
        let icon = theme::icon_px(14.0);
        let indent = icon + 4.0;
        let t = 1.5 + theme::band().sqrt();
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            // 見出し: 開閉の印、名前、数
            ui.horizontal(|ui| {
                ui.set_height(row_h);
                ui.spacing_mut().item_spacing.x = 4.0;
                let (tw, tr) = ui.allocate_exact_size(Vec2::splat(icon), Sense::click());
                self.group_toggle(ui, g, tw, tr);
                if self
                    .cur
                    .renaming_group
                    .as_ref()
                    .is_some_and(|(id, _)| *id == g.id)
                {
                    let (r, _) = ui.allocate_exact_size(
                        Vec2::new(theme::fs(110.0), theme::fs(11.0) * 1.25 + 6.0),
                        Sense::hover(),
                    );
                    self.group_rename_box(ui, g.id, r);
                } else {
                    let r = ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new(&g.name)
                                    .size(crate::theme::fs(10.5))
                                    .color(color)
                                    .strong(),
                            )
                            .sense(Sense::click()),
                        )
                        .on_hover_text("押すと中身をまとめて選ぶ。右クリックで名前・解く・消す");
                    if r.clicked() {
                        self.select_group_members(data, g.id);
                    }
                    if r.double_clicked() {
                        self.cur.collapsed.remove(&g.id);
                    }
                    r.context_menu(|ui| self.group_menu(ui, g));
                }
                ui.label(
                    egui::RichText::new((e - s + 1).to_string())
                        .size(crate::theme::fs(10.5))
                        .color(l.pal.ink3),
                );
                // 畳んでいても、中に問題があれば印を出す(エラーがあればエラーの印)
                let worst = worst_health(
                    data.features[s..=e]
                        .iter()
                        .map(|f| Health::of(self.cur.work.report().feature(f.id))),
                );
                if collapsed && let Some(h) = worst {
                    let (d, r) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                    paint_health(ui.painter(), d.center(), &h, &l, false);
                    r.on_hover_text(format!("中に{}があります(開くと見える)", h.title()));
                }
            });
            if collapsed {
                return;
            }
            let body = ui.horizontal(|ui| {
                ui.add_space(indent);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    self.timeline_range(
                        ui,
                        data,
                        (s, e + 1),
                        path,
                        Rows::default(),
                        chips,
                        &mut Vec::new(),
                    );
                });
            });
            let r = body.response.rect;
            let x = r.left() + icon / 2.0;
            let w = if has_sel { t * 1.8 } else { t };
            ui.painter().rect_filled(
                Rect::from_x_y_ranges(
                    egui::Rangef::new(x - w / 2.0, x + w / 2.0),
                    egui::Rangef::new(r.top() + 1.0, r.bottom() - 1.0),
                ),
                w / 2.0,
                color,
            );
        });
    }

    /// 機能の種類のアイコン(記述子の名前。なければ区分から)。
    pub(crate) fn feature_icon_name(&self, type_id: &str) -> String {
        match self.win.registry.descriptor(type_id) {
            Some(d) if !d.icon.is_empty() => d.icon,
            Some(d) => match d.category.as_str() {
                kmb_core::category::SKETCH => "sketch".into(),
                kmb_core::category::SETUP => "setup".into(),
                kmb_core::category::OPERATION => "path".into(),
                kmb_core::category::TOOLS => "tools".into(),
                _ => "box".into(),
            },
            None => "box".into(),
        }
    }

    fn chip(
        &mut self,
        ui: &mut egui::Ui,
        data: &kmb_core::DocumentData,
        i: usize,
        rows: Rows,
        chips: &mut Vec<(FeatureId, egui::Rect)>,
    ) {
        let l = theme::look(ui.ctx());
        let f = &data.features[i];
        let health = self.health(f.id);
        let selected = self.selection().contains(&f.id);
        let rolled = i >= data.active_len();
        let icon = self.feature_icon_name(&f.type_id);
        let cam = self.win.registry.descriptor(&f.type_id).is_some_and(|d| {
            matches!(
                d.category.as_str(),
                kmb_core::category::OPERATION | kmb_core::category::SETUP | kmb_core::category::TOOLS
            )
        });
        let v = self.win.timeline_vertical;
        // 縦の一覧の行は、アイコンと名前が入る高さ(1 倍で 24)
        let row_icon = theme::icon_px(16.0);
        let size = if v {
            Vec2::new(
                ui.available_width(),
                24.0_f32
                    .max(row_icon + 8.0)
                    .max(theme::fs(12.5) * 1.3 + 6.0),
            )
        } else {
            // 広げた時は、札の下に道具の名前を書く(長い名前は … で切る)
            let w = if rows.name > 0.0 {
                let text = ui
                    .painter()
                    .layout_no_wrap(f.name.clone(), theme::prop(10.5), l.pal.ink2)
                    .size()
                    .x;
                (text + 8.0).min(theme::fs(84.0)).max(chip_size().x)
            } else {
                chip_size().x
            };
            Vec2::new(w, rows.item_h())
        };
        let (full, r) = ui.allocate_exact_size(size, Sense::click_and_drag());
        r.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &f.name)
        });
        // 札の見える所(横は、上のグループの名前の行と下の下線の分を除く)
        let rect = if v {
            full
        } else {
            Rect::from_min_size(
                full.min + Vec2::new(0.0, rows.top),
                Vec2::new(full.width(), chip_size().y + rows.name),
            )
        };
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, crate::theme::corner(4.0), l.pal.sel);
        } else if r.hovered() {
            p.rect_filled(rect, crate::theme::corner(4.0), l.pal.chrome2);
        }
        // 問題のある機能は札ごと黄色(注意)か赤(エラー)にする。前の機能のせいで止まった物は赤の枠だけ
        if health.is_problem() && !rolled {
            let c = health.color(&l);
            if !matches!(health, Health::Blocked { .. }) {
                let base = if selected { l.pal.sel } else { l.pal.panel };
                p.rect_filled(rect, crate::theme::corner(4.0), theme::mix(base, c, 0.22));
            }
            let w = if health.is_red() { 1.5 } else { 1.2 };
            p.rect_stroke(
                rect,
                crate::theme::corner(4.0),
                Stroke::new(w, c),
                egui::StrokeKind::Inside,
            );
        }
        let mut c = if cam {
            l.pal.cam
        } else if selected {
            l.pal.ink
        } else {
            l.pal.ink2
        };
        if rolled || f.suppressed {
            c = c.gamma_multiply(0.4);
        }
        // 縦の一覧の行は、左にアイコン、続けて名前、右の端に具合の印
        let icon_at = if v {
            egui::pos2(rect.left() + 6.0 + row_icon / 2.0, rect.center().y)
        } else {
            egui::pos2(rect.center().x, rect.top() + chip_size().y / 2.0)
        };
        icons::paint_exact(
            p,
            Rect::from_center_size(
                icon_at,
                Vec2::splat(if v { row_icon } else { theme::icon_px(18.0) }),
            ),
            &icon,
            c,
            1.6,
        );
        if v {
            let ink = if rolled || f.suppressed {
                l.pal.ink3
            } else {
                l.pal.ink
            };
            let galley = p.layout(
                f.name.clone(),
                crate::theme::prop(12.5),
                ink,
                (rect.width() - 32.0 - row_icon).max(10.0),
            );
            let text_rect = Rect::from_min_max(
                egui::pos2(rect.left() + 12.0 + row_icon, rect.top()),
                egui::pos2(rect.right() - 18.0, rect.bottom()),
            );
            p.with_clip_rect(text_rect).galley(
                egui::pos2(text_rect.left(), rect.center().y - galley.size().y / 2.0),
                galley,
                ink,
            );
        }
        if !v && rows.name > 0.0 {
            let ink = if rolled || f.suppressed {
                l.pal.ink3
            } else {
                l.pal.ink2
            };
            let mut job =
                egui::text::LayoutJob::simple_singleline(f.name.clone(), theme::prop(10.5), ink);
            job.wrap = egui::text::TextWrapping {
                max_width: rect.width() - 4.0,
                max_rows: 1,
                break_anywhere: true,
                overflow_character: Some('…'),
            };
            let galley = p.layout_job(job);
            let y = rect.top() + chip_size().y - 4.0 + (rows.name - galley.size().y) / 2.0;
            p.galley(
                egui::pos2(rect.center().x - galley.size().x / 2.0, y),
                galley,
                ink,
            );
        }
        // 具合の印は、横はアイコンの所の右下
        let icon_box = if v {
            rect
        } else {
            Rect::from_min_size(rect.min, Vec2::new(rect.width(), chip_size().y))
        };
        let at = if v {
            egui::pos2(rect.right() - 10.0, rect.center().y)
        } else if health.is_problem() {
            icon_box.right_bottom() + Vec2::new(-7.0, -7.0)
        } else {
            icon_box.right_bottom() + Vec2::new(-6.0, -6.0)
        };
        paint_health(p, at, &health, &l, rolled);
        let reused = matches!(
            self.cur.work.report().feature(f.id),
            Some(FeatureStatus::Ok { reused: true, .. })
        );
        let tip = chip_tip(
            &f.name,
            &health,
            self.cur.work.report().time_ms(f.id),
            reused,
        );
        // 「タイムライン内を検索」で探した機能は、しばらく差し色の枠で光らせ、見える所まで送る。
        // AI が足した・直した機能も同じ枠で光らせる
        if let Some(glow) = self
            .found_glow(ui.ctx(), Found::Timeline(f.id))
            .or_else(|| self.ai_glow(ui.ctx(), f.id, None))
        {
            p.rect_stroke(
                rect.expand(1.0),
                5.0,
                Stroke::new(2.5, l.ws().gamma_multiply(glow)),
                egui::StrokeKind::Outside,
            );
            if self
                .cur
                .found_scroll
                .take_if(|x| *x == Found::Timeline(f.id))
                .is_some()
            {
                r.scroll_to_me(Some(egui::Align::Center));
            }
        }
        let r = r.on_hover_text(tip);
        chips.push((f.id, r.rect));
        if r.double_clicked() {
            self.open_edit(f.id);
        } else if r.clicked() {
            let m = ui.input(|i| i.modifiers);
            if m.command {
                self.toggle_select(f.id);
            } else if m.shift {
                self.select_range(f.id);
            } else {
                self.select(f.id);
            }
        }
        if r.drag_started() {
            let key = crate::doc_id(ui.ctx(), "timeline_drag");
            ui.ctx().data_mut(|d| d.insert_temp(key, Dragging(f.id)));
        }
        r.context_menu(|ui| {
            if !self.selection().contains(&f.id) {
                self.select(f.id);
            }
            self.feature_menu(ui, f.id);
            // ボディを作る機能は、そのボディをブラウザで探せる(3D のビューの右クリックと同じ)
            if self.output(f.id, "body").is_some()
                && ui
                    .button("ブラウザ内を検索")
                    .on_hover_text("この機能のボディを、ブラウザで光らせる")
                    .clicked()
            {
                self.find_in_browser(f.id);
                ui.close();
            }
        });
    }

    /// ドラッグを離した所の機能の位置へ動かす。
    fn finish_drag(
        &mut self,
        ui: &egui::Ui,
        data: &kmb_core::DocumentData,
        chips: &[(FeatureId, egui::Rect)],
    ) {
        let key = crate::doc_id(ui.ctx(), "timeline_drag");
        let Some(Dragging(id)) = ui.ctx().data(|d| d.get_temp::<Dragging>(key)) else {
            return;
        };
        // 選んだ中の機能を引いた時は、選んだ機能をまとめて動かす(U45)
        let sel = self.selection();
        let group: Vec<FeatureId> = if sel.len() > 1 && sel.contains(&id) {
            sel
        } else {
            vec![id]
        };
        let pos = ui.input(|i| i.pointer.latest_pos());
        if let Some(p) = pos {
            let label = match group.len() {
                1 => data.feature(id).map_or(String::new(), |f| f.name.clone()),
                n => format!("{n} 個の機能"),
            };
            ui.painter().text(
                p + egui::vec2(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                label,
                crate::theme::prop(12.0),
                ui.visuals().strong_text_color(),
            );
        }
        if ui.input(|i| i.pointer.any_down()) {
            // 引いている間は、落とす先に線を引く(文書のタブの並べ替えと同じ見せ方)
            if let Some((a, b)) = pos.and_then(|p| drop_line(self.win.timeline_vertical, chips, p)) {
                let ink = egui::Stroke::new(2.0, theme::look(ui.ctx()).ws());
                ui.painter().line_segment([a, b], ink);
            }
            return;
        }
        ui.ctx().data_mut(|d| d.remove::<Dragging>(key));
        let (Some(p), Some(from)) = (pos, data.index_of(id)) else {
            return;
        };
        let Some((target, rect)) = chips.iter().find(|(_, r)| r.contains(p)) else {
            return;
        };
        if group.len() > 1 {
            let after = along(self.win.timeline_vertical, p)
                > along(self.win.timeline_vertical, rect.center());
            match edit::move_features(data, &self.win.registry, &group, *target, after) {
                Ok(Some(tx)) => {
                    self.apply(tx);
                }
                Ok(None) => {}
                Err(why) => self.win.fail(why),
            }
            return;
        }
        let Some(mut to) = data.index_of(*target) else {
            return;
        };
        // 右半分(縦の一覧は下半分)に落としたら、その機能の後ろへ
        let after =
            along(self.win.timeline_vertical, p) > along(self.win.timeline_vertical, rect.center());
        if after && to < from {
            to += 1;
        } else if !after && to > from {
            to -= 1;
        }
        if to != from {
            if let Some(why) = edit::move_conflict(data, &self.win.registry, id, to) {
                self.win.fail(why);
            } else if let Some(tx) = edit::move_feature(data, id, to as i32 - from as i32) {
                self.apply(tx);
            }
        }
    }

    /// 選んだ機能をグループにする。続いていない時は理由と、間も含める・寄せるの問いを出す。
    pub fn group_features(&mut self, ids: &[FeatureId], mode: GroupMode) -> Option<u64> {
        let name = format!("グループ{}", self.cur.work.doc().data().groups.len() + 1);
        let registry = &self.win.registry;
        match self
            .cur
            .work
            .prepare(|d| edit::make_group(d, registry, ids, &name, mode))
        {
            Ok(tx) => {
                let before: Vec<u64> = self
                    .cur
                    .work
                    .doc()
                    .data()
                    .groups
                    .iter()
                    .map(|g| g.id)
                    .collect();
                if !self.apply(tx) {
                    return None;
                }
                let new = self
                    .cur
                    .work
                    .doc()
                    .data()
                    .groups
                    .iter()
                    .find(|g| !before.contains(&g.id))
                    .map(|g| (g.id, g.name.clone()));
                if let Some((gid, name)) = &new {
                    // すぐ名前を打てるようにする
                    self.cur.renaming_group = Some((*gid, name.clone()));
                }
                self.cur.group_ask = None;
                new.map(|n| n.0)
            }
            Err(e @ (GroupError::Gaps(_) | GroupError::Order(_))) => {
                self.cur.group_ask = Some((ids.to_vec(), e));
                None
            }
            Err(GroupError::Other(m)) => {
                self.win.fail(m);
                None
            }
        }
    }

    /// グループにできなかった理由と、どうするかの問い。
    pub(crate) fn group_dialog(&mut self, ctx: &egui::Context) {
        let Some((ids, err)) = self.cur.group_ask.clone() else {
            return;
        };
        let why = err.describe(self.cur.work.doc().data());
        let mut choice = None;
        let mut open = true;
        egui::Window::new("グループにできません")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(&why);
                ui.horizontal(|ui| {
                    if matches!(err, GroupError::Gaps(_)) {
                        if ui
                            .button("間も含める")
                            .on_hover_text("最初から最後までの間にある機能も入れてまとめる")
                            .clicked()
                        {
                            choice = Some(GroupMode::IncludeGaps);
                        }
                        if ui
                            .button("寄せてからまとめる")
                            .on_hover_text("間の機能を後ろへ出し、選んだものだけを並べてまとめる")
                            .clicked()
                        {
                            choice = Some(GroupMode::Gather);
                        }
                    }
                    if ui.button("キャンセル").clicked() {
                        self.cur.group_ask = None;
                    }
                });
            });
        if !open {
            self.cur.group_ask = None;
        }
        if let Some(mode) = choice {
            self.cur.group_ask = None;
            self.group_features(&ids, mode);
        }
    }

    /// グループの見出しの右クリック。
    fn group_menu(&mut self, ui: &mut egui::Ui, g: &Group) {
        let data = self.cur.work.doc().data().clone();
        let members = edit::group_members(&data, g.id);
        let mut done = false;
        if ui.button("名前変更").clicked() {
            self.cur.renaming_group = Some((g.id, g.name.clone()));
            done = true;
        }
        let collapsed = self.cur.collapsed.contains(&g.id);
        if ui.button(if collapsed { "開く" } else { "畳む" }).clicked() {
            if collapsed {
                self.cur.collapsed.remove(&g.id);
            } else {
                self.cur.collapsed.insert(g.id);
            }
            done = true;
        }
        let all_suppressed = members
            .iter()
            .all(|id| data.feature(*id).is_some_and(|f| f.suppressed));
        if ui
            .button(if all_suppressed {
                "まとめて抑制を解除"
            } else {
                "まとめて抑制"
            })
            .clicked()
        {
            self.apply(edit::set_suppressed(&members, !all_suppressed));
            done = true;
        }
        ui.separator();
        if ui.button("グループを解く(中身は残す)").clicked()
            && let Ok(tx) = edit::ungroup(&data, g.id)
        {
            self.apply(tx);
            done = true;
        }
        let red = ui.visuals().error_fg_color;
        if ui
            .button(egui::RichText::new("グループと中身を削除").color(red))
            .on_hover_text(format!(
                "中の {} 個の機能も消す。取り消し(Ctrl+Z)で戻せる",
                members.len()
            ))
            .clicked()
            && let Ok(tx) = edit::delete_group(&data, g.id)
        {
            if self.selection().iter().any(|s| members.contains(s)) {
                self.clear_selection();
            }
            self.apply(tx);
            done = true;
        }
        if done {
            ui.close();
        }
    }
}

#[cfg(test)]
mod health_tests {
    use super::*;

    #[test]
    fn health_tells_errors_blocked_and_warnings_apart() {
        let mut out = kmb_core::Outputs::default();
        assert_eq!(
            Health::of(Some(&FeatureStatus::Ok {
                outputs: out.clone(),
                reused: false
            })),
            Health::Ok
        );
        kmb_core::add_warnings(&mut out, vec!["タブがありません".into()]);
        let warn = Health::of(Some(&FeatureStatus::Ok {
            outputs: out,
            reused: false,
        }));
        assert_eq!(warn, Health::Warning("タブがありません".into()));
        assert!(warn.is_problem() && !warn.is_red());
        let err = Health::of(Some(&FeatureStatus::Error {
            message: "半径が大きすぎます".into(),
        }));
        assert!(err.is_red());
        let blocked = Health::of(Some(&FeatureStatus::Blocked {
            by: "フィレット1".into(),
            feature: Some(FeatureId(3)),
        }));
        assert_eq!(
            blocked.message(),
            "フィレット1 が使えないので、この機能は計算していません"
        );
        assert_eq!(Health::of(Some(&FeatureStatus::Suppressed)), Health::Idle);
        assert!(!Health::Idle.is_problem());
        // いちばん重い物: エラー > 止まっている > 注意
        let all = [warn.clone(), blocked.clone(), err.clone(), Health::Ok];
        assert_eq!(worst_health(all.into_iter()), Some(err.clone()));
        assert_eq!(
            worst_health([warn.clone(), blocked.clone()].into_iter()),
            Some(blocked)
        );
        assert_eq!(worst_health([Health::Ok, Health::Idle].into_iter()), None);
        // 指を置いた時の説明に、種類と文と確かめ方
        let tip = health_tip("フィレット2", &err);
        assert!(tip.starts_with("フィレット2\nエラー: 半径が大きすぎます"));
        assert!(tip.contains("「エラーを確かめる」"));
        assert!(health_tip("押し出し1", &Health::Ok) == "押し出し1");
        assert!(health_tip("外形", &warn).contains("「注意を確かめる」"));
    }
}

#[cfg(test)]
mod drop_line_tests {
    use super::*;

    #[test]
    fn drop_line_is_on_the_edge_the_feature_lands_next_to() {
        let r = Rect::from_min_size(egui::pos2(10.0, 0.0), egui::vec2(20.0, 30.0));
        let chips = [(FeatureId(1), r)];
        // 横の並び: 左半分は左の縁、右半分は右の縁に縦の線
        let (a, b) = drop_line(false, &chips, egui::pos2(12.0, 5.0)).unwrap();
        assert_eq!((a, b), (egui::pos2(10.0, 0.0), egui::pos2(10.0, 30.0)));
        let (a, _) = drop_line(false, &chips, egui::pos2(28.0, 5.0)).unwrap();
        assert_eq!(a.x, 30.0);
        // 縦の一覧: 下半分は下の縁に横の線
        let (a, b) = drop_line(true, &chips, egui::pos2(15.0, 25.0)).unwrap();
        assert_eq!((a, b), (egui::pos2(10.0, 30.0), egui::pos2(30.0, 30.0)));
        // 機能の上でなければ線を引かない
        assert!(drop_line(false, &chips, egui::pos2(50.0, 5.0)).is_none());
    }
}
